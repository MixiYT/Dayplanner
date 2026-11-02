package app.dayplanner.nativebridge

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import org.json.JSONArray
import org.json.JSONObject

object Scheduler {
    const val ACTION_FIRE = "app.dayplanner.ACTION_FIRE"

    fun canExact(am: AlarmManager): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.S || am.canScheduleExactAlarms()

    private fun fireIntent(c: Context, key: String): Intent =
        Intent(c, AlarmReceiver::class.java)
            .setAction(ACTION_FIRE)
            .setData(Uri.parse("planner://alarm/" + Uri.encode(key)))

    private fun pending(c: Context, a: JSONObject): PendingIntent {
        val key = a.getString("key")
        val i = fireIntent(c, key)
            .putExtra("key", key)
            .putExtra("reminder_id", a.optLong("reminder_id"))
            .putExtra("at", a.optString("at"))
            .putExtra("title", a.optString("title"))
            .putExtra("notes", a.optString("notes"))
            .putExtra("icon", a.optString("icon", "🔔"))
            .putExtra("alert", a.optString("alert", "notification"))
        return PendingIntent.getBroadcast(
            c, 0, i, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
    }

    private fun showApp(c: Context): PendingIntent? {
        val launch = c.packageManager.getLaunchIntentForPackage(c.packageName) ?: return null
        return PendingIntent.getActivity(c, 1, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    private fun setOne(c: Context, a: JSONObject, triggerAt: Long) {
        val am = c.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        val p = pending(c, a)
        try {
            when {
                // Будильник идёт как «системный будильник»: срабатывает точно и в режиме сна.
                a.optString("alert") == "alarm" ->
                    am.setAlarmClock(AlarmManager.AlarmClockInfo(triggerAt, showApp(c) ?: p), p)
                canExact(am) -> am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAt, p)
                else -> am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAt, p)
            }
        } catch (e: SecurityException) {
            am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAt, p)
        }
    }

    private fun cancelKey(c: Context, key: String) {
        val am = c.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        val p = PendingIntent.getBroadcast(
            c, 0, fireIntent(c, key), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        am.cancel(p)
        p.cancel()
    }

    /** Заменяет все запланированные напоминания новым списком из ядра. */
    @Synchronized
    fun replaceAll(c: Context, alarms: JSONArray) {
        Prefs.scheduledKeys(c).forEach { cancelKey(c, it) }
        val keys = ArrayList<String>()
        val now = System.currentTimeMillis()
        for (i in 0 until alarms.length()) {
            val a = alarms.optJSONObject(i) ?: continue
            val t = TimeFmt.parse(a.optString("at")) ?: continue
            if (t <= now || !a.has("key")) continue
            setOne(c, a, t)
            keys.add(a.getString("key"))
        }
        Prefs.saveScheduledKeys(c, keys)
        Prefs.saveAlarms(c, alarms.toString())
    }

    /** После перезагрузки: ставим то, что ещё не наступило. */
    fun restore(c: Context) = replaceAll(c, Prefs.alarms(c))

    /** Отложить напоминание: разовый повтор через [minutes] минут. */
    fun snooze(c: Context, ex: Bundle, minutes: Int) {
        val a = JSONObject()
            .put("key", (ex.getString("key") ?: "snooze") + "|snooze")
            .put("reminder_id", ex.getLong("reminder_id"))
            .put("at", ex.getString("at") ?: "")
            .put("title", ex.getString("title") ?: "")
            .put("notes", ex.getString("notes") ?: "")
            .put("icon", ex.getString("icon") ?: "🔔")
            .put("alert", ex.getString("alert") ?: "notification")
        setOne(c, a, System.currentTimeMillis() + minutes * 60_000L)
    }
}
