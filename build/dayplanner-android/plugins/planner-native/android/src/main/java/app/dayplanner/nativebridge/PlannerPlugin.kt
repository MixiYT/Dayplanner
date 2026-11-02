package app.dayplanner.nativebridge

import android.Manifest
import android.app.Activity
import android.app.AlarmManager
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import android.webkit.WebView
import android.util.Base64
import androidx.activity.result.ActivityResult
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.tauri.annotation.Command
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONArray

/**
 * Команды, которые интерфейс вызывает через Rust: `native("sync" | "status" | "request" | "drain", {...})`.
 */
@TauriPlugin
class PlannerPlugin(private val activity: Activity) : Plugin(activity) {

    @Command
    fun lan_status(invoke: Invoke) {
        val res = JSObject()
        val granted = Build.VERSION.SDK_INT < 37 || ContextCompat.checkSelfPermission(activity, "android.permission.ACCESS_LOCAL_NETWORK") == PackageManager.PERMISSION_GRANTED
        res.put("granted", granted)
        invoke.resolve(res)
    }
    private var webView: WebView? = null

    @Command
    fun open_url(invoke: Invoke) {
        val url = invoke.getArgs().optString("url", "")
        if (!url.startsWith("https://www.dropbox.com/oauth2/authorize?")) { invoke.reject("Недопустимый адрес"); return }
        try { activity.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url))); invoke.resolve() }
        catch (e: Exception) { invoke.reject("Не удалось открыть браузер") }
    }

    @Command
    fun save_file(invoke: Invoke) {
        val args = invoke.getArgs()
        if (args.optString("data").length > 56 * 1024 * 1024) { invoke.reject("Файл больше 40 МБ"); return }
        val name = args.optString("name", "file").substringAfterLast('/').substringAfterLast('\\').take(200)
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "application/octet-stream"
            putExtra(Intent.EXTRA_TITLE, name)
        }
        try { startActivityForResult(invoke, intent, "savedFile") }
        catch (e: Exception) { invoke.reject("Не удалось открыть выбор файла") }
    }

    @ActivityCallback
    private fun savedFile(invoke: Invoke, result: ActivityResult) {
        val uri = result.data?.data
        if (result.resultCode != Activity.RESULT_OK || uri == null) { val res = JSObject(); res.put("saved", false); invoke.resolve(res); return }
        Thread {
            try {
                val bytes = Base64.decode(invoke.getArgs().optString("data"), Base64.DEFAULT)
                activity.contentResolver.openOutputStream(uri, "w")?.use { it.write(bytes) }
                    ?: throw IllegalStateException("Не удалось открыть файл")
                val res = JSObject(); res.put("saved", true); invoke.resolve(res)
            } catch (e: Exception) { invoke.reject("Не удалось сохранить файл") }
        }.start()
    }

    override fun load(webView: WebView) {
        this.webView = webView
        // Recheck after rotation, split-screen and keyboard changes without replacing
        // Tauri's own inset listener or consuming the system insets.
        webView.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
            webView.evaluateJavascript("window.dispatchEvent(new Event('planner-insets'))", null)
        }
    }

    /** Only the parts of system bars/cutouts actually overlapping this WebView. */
    @Command
    fun insets(invoke: Invoke) {
        activity.runOnUiThread {
            val view = webView
            val root = activity.window.decorView
            val windowInsets = view?.let { ViewCompat.getRootWindowInsets(it) }
            if (view == null || view.width == 0 || windowInsets == null) {
                invoke.resolve()
                return@runOnUiThread
            }
            val bars = windowInsets.getInsets(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
            )
            val position = IntArray(2)
            val origin = IntArray(2)
            view.getLocationOnScreen(position)
            root.getLocationOnScreen(origin)
            val x = position[0] - origin[0]
            val y = position[1] - origin[1]
            val res = JSObject()
            res.put("width", view.width)
            res.put("top", (bars.top - y).coerceAtLeast(0))
            res.put("right", (x + view.width - root.width + bars.right).coerceAtLeast(0))
            res.put("bottom", (y + view.height - root.height + bars.bottom).coerceAtLeast(0))
            res.put("left", (bars.left - x).coerceAtLeast(0))
            invoke.resolve(res)
        }
    }

    /** Принять от ядра список будильников и данные для виджетов. */
    @Command
    fun sync(invoke: Invoke) {
        try {
            val args = invoke.getArgs()
            Prefs.saveWidget(activity, args.optString("widget", "{}"))
            Notifier.ensureChannels(activity)
            Scheduler.replaceAll(activity, JSONArray(args.optString("alarms", "[]")))
            WidgetRenderer.updateAll(activity)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject(e.message ?: "Не удалось обновить будильники")
        }
    }

    /** Какие разрешения уже выданы. */
    @Command
    fun status(invoke: Invoke) {
        val am = activity.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        val nm = activity.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val pm = activity.getSystemService(Context.POWER_SERVICE) as PowerManager
        val notifications = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            ContextCompat.checkSelfPermission(activity, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        } else {
            NotificationManagerCompat.from(activity).areNotificationsEnabled()
        }
        val fullScreen = if (Build.VERSION.SDK_INT >= 34) nm.canUseFullScreenIntent() else true
        val res = JSObject()
        res.put("notifications", notifications)
        res.put("exactAlarms", Scheduler.canExact(am))
        res.put("fullScreen", fullScreen)
        res.put("batteryOk", pm.isIgnoringBatteryOptimizations(activity.packageName))
        invoke.resolve(res)
    }

    /** Запросить разрешение или открыть нужный экран настроек. */
    @Command
    fun request(invoke: Invoke) {
        val kind = invoke.getArgs().optString("kind", "")
        val pkg = Uri.parse("package:" + activity.packageName)
        try {
            when (kind) {
                "notifications" ->
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7001)
                    } else {
                        open(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName))
                    }
                "exactAlarms" ->
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) open(Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, pkg))
                "fullScreen" ->
                    if (Build.VERSION.SDK_INT >= 34) open(Intent(Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT, pkg))
                "battery" -> open(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, pkg))
                "localNetwork" -> if (Build.VERSION.SDK_INT >= 37 && ContextCompat.checkSelfPermission(activity, "android.permission.ACCESS_LOCAL_NETWORK") != PackageManager.PERMISSION_GRANTED)
                    activity.requestPermissions(arrayOf("android.permission.ACCESS_LOCAL_NETWORK"), 7002)
            }
            invoke.resolve()
        } catch (e: Exception) {
            // На некоторых прошивках нужного экрана нет — открываем общие настройки приложения.
            try {
                open(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, pkg))
                invoke.resolve()
            } catch (e2: Exception) {
                invoke.reject(e2.message ?: "Не удалось открыть настройки")
            }
        }
    }

    /** Забрать отметки, сделанные в уведомлениях, пока приложение было закрыто. */
    @Command
    fun drain(invoke: Invoke) {
        val res = JSObject()
        res.put("items", JSArray(Prefs.drain(activity).toString()))
        invoke.resolve(res)
    }

    private fun open(i: Intent) {
        activity.startActivity(i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
}
