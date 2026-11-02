package app.dayplanner.nativebridge

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

object Notifier {
    const val CH_REMINDER = "pn_reminders"
    const val CH_FULL = "pn_fullscreen"
    const val CH_ALARM = "pn_alarm"

    const val ACTION_DONE = "app.dayplanner.ACTION_DONE"
    const val ACTION_SKIP = "app.dayplanner.ACTION_SKIP"
    const val ACTION_SNOOZE = "app.dayplanner.ACTION_SNOOZE"
    const val ACTION_STOP = "app.dayplanner.ACTION_STOP"

    fun notifId(ex: Bundle): Int = (ex.getString("key") ?: "").hashCode()

    fun ensureChannels(c: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        nm.createNotificationChannel(
            NotificationChannel(CH_REMINDER, "Напоминания", NotificationManager.IMPORTANCE_HIGH)
        )
        nm.createNotificationChannel(
            NotificationChannel(CH_FULL, "Напоминания на весь экран", NotificationManager.IMPORTANCE_HIGH)
        )
        // Звук будильника играет сервис, поэтому у канала собственного звука нет.
        nm.createNotificationChannel(
            NotificationChannel(CH_ALARM, "Будильники", NotificationManager.IMPORTANCE_HIGH).apply {
                setSound(null, null)
                enableVibration(false)
            }
        )
    }

    private fun openApp(c: Context): PendingIntent {
        val launch = c.packageManager.getLaunchIntentForPackage(c.packageName) ?: Intent()
        return PendingIntent.getActivity(c, 2, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    fun actionIntent(c: Context, action: String, ex: Bundle): PendingIntent {
        val i = Intent(c, ActionReceiver::class.java)
            .setAction(action)
            .setData(Uri.parse("planner://action/" + action + "/" + Uri.encode(ex.getString("key") ?: "")))
            .putExtras(ex)
        return PendingIntent.getBroadcast(c, 0, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    fun alertScreen(c: Context, ex: Bundle): PendingIntent {
        val i = Intent(c, AlertActivity::class.java)
            .setData(Uri.parse("planner://alert/" + Uri.encode(ex.getString("key") ?: "")))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_USER_ACTION)
            .putExtras(ex)
        return PendingIntent.getActivity(c, 3, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    fun build(c: Context, ex: Bundle, channel: String, fullScreen: Boolean, ongoing: Boolean): Notification {
        val icon = ex.getString("icon") ?: "🔔"
        val title = PlannerIcon.text(icon) + " " + (ex.getString("title") ?: "")
        val notes = ex.getString("notes") ?: ""
        val time = TimeFmt.hhmm(ex.getString("at") ?: "")
        val text = if (notes.isBlank()) "Сейчас $time" else "$time · $notes"
        val b = NotificationCompat.Builder(c, channel)
            .setSmallIcon(R.drawable.pn_ic_notify)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(if (channel == CH_ALARM) NotificationCompat.CATEGORY_ALARM else NotificationCompat.CATEGORY_REMINDER)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(openApp(c))
            .setAutoCancel(!ongoing)
            .setOngoing(ongoing)
            .addAction(0, "Выполнено", actionIntent(c, ACTION_DONE, ex))
        if (channel == CH_ALARM) {
            b.addAction(0, "Выключить", actionIntent(c, ACTION_STOP, ex))
        } else {
            b.addAction(0, "Пропустить", actionIntent(c, ACTION_SKIP, ex))
            b.addAction(0, "Через 10 мин", actionIntent(c, ACTION_SNOOZE, ex))
        }
        PlannerIcon.bitmap(icon)?.let { b.setLargeIcon(it) }
        if (fullScreen) b.setFullScreenIntent(alertScreen(c, ex), true)
        return b.build()
    }

    fun show(c: Context, ex: Bundle, fullScreen: Boolean) {
        ensureChannels(c)
        val n = build(c, ex, if (fullScreen) CH_FULL else CH_REMINDER, fullScreen, false)
        try {
            NotificationManagerCompat.from(c).notify(notifId(ex), n)
        } catch (e: SecurityException) {
            // Нет разрешения на уведомления — пользователь увидит подсказку в настройках.
        }
    }
}
