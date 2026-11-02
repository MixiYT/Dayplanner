package app.dayplanner.nativebridge

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

object PlannerIcon {
    fun text(value: String): String = if (value.startsWith("data:")) "🖼" else value.take(64)
    fun bitmap(value: String): Bitmap? = try {
        if (!value.startsWith("data:image/png;base64,") || value.length > 350050) null
        else {
            val bytes = Base64.decode(value.substringAfter(','), Base64.DEFAULT)
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            if (bounds.outWidth !in 1..192 || bounds.outHeight !in 1..192) null
            else BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
        }
    } catch (e: Exception) { null }
}

/** Время в том же формате, что и в ядре: локальное, без часового пояса. */
object TimeFmt {
    private const val PATTERN = "yyyy-MM-dd'T'HH:mm:ss"
    fun parse(s: String): Long? = try {
        SimpleDateFormat(PATTERN, Locale.US).parse(s)?.time
    } catch (e: Exception) {
        null
    }

    fun now(): String = SimpleDateFormat(PATTERN, Locale.US).format(Date())
    fun today(): String = SimpleDateFormat("yyyy-MM-dd", Locale.US).format(Date())
    fun hhmm(iso: String): String = if (iso.length >= 16) iso.substring(11, 16) else iso
}

/** Всё, что нужно помнить, пока основное приложение закрыто. */
object Prefs {
    private fun sp(c: Context) = c.getSharedPreferences("planner_native", Context.MODE_PRIVATE)

    fun saveWidget(c: Context, json: String) = sp(c).edit().putString("widget", json).apply()
    fun widget(c: Context): JSONObject = try {
        JSONObject(sp(c).getString("widget", "{}") ?: "{}")
    } catch (e: Exception) {
        JSONObject()
    }

    fun saveAlarms(c: Context, json: String) = sp(c).edit().putString("alarms", json).apply()
    fun alarms(c: Context): JSONArray = try {
        JSONArray(sp(c).getString("alarms", "[]") ?: "[]")
    } catch (e: Exception) {
        JSONArray()
    }

    fun scheduledKeys(c: Context): List<String> = try {
        val a = JSONArray(sp(c).getString("keys", "[]") ?: "[]")
        (0 until a.length()).map { a.getString(it) }
    } catch (e: Exception) {
        emptyList()
    }

    fun saveScheduledKeys(c: Context, keys: List<String>) =
        sp(c).edit().putString("keys", JSONArray(keys).toString()).apply()

    /** Отметка, сделанная в уведомлении. Интерфейс заберёт её при следующем запуске. */
    @Synchronized
    fun pushAction(c: Context, kind: String, reminderId: Long, at: String) {
        val arr = try {
            JSONArray(sp(c).getString("actions", "[]") ?: "[]")
        } catch (e: Exception) {
            JSONArray()
        }
        arr.put(
            JSONObject()
                .put("kind", kind)
                .put("reminder_id", reminderId)
                .put("at", at)
                .put("ts", TimeFmt.now())
        )
        sp(c).edit().putString("actions", arr.toString()).apply()
    }

    @Synchronized
    fun drain(c: Context): JSONArray {
        val s = sp(c).getString("actions", "[]") ?: "[]"
        sp(c).edit().putString("actions", "[]").apply()
        return try {
            JSONArray(s)
        } catch (e: Exception) {
            JSONArray()
        }
    }
}
