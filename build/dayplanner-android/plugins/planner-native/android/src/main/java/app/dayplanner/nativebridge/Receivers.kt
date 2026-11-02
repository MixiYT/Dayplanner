package app.dayplanner.nativebridge

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat

/** Сработал таймер: показываем напоминание выбранным способом. */
class AlarmReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val ex = intent.extras ?: return
        when (ex.getString("alert")) {
            "alarm" -> ContextCompat.startForegroundService(
                context, Intent(context, AlarmService::class.java).putExtras(ex)
            )
            "full_screen" -> Notifier.show(context, ex, fullScreen = true)
            else -> Notifier.show(context, ex, fullScreen = false)
        }
    }
}

/** Кнопки в уведомлении и на экране напоминания. */
class ActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val ex = intent.extras ?: return
        NotificationManagerCompat.from(context).cancel(Notifier.notifId(ex))
        context.stopService(Intent(context, AlarmService::class.java))
        val id = ex.getLong("reminder_id")
        val at = ex.getString("at") ?: return
        when (intent.action) {
            Notifier.ACTION_DONE -> Prefs.pushAction(context, "complete", id, at)
            Notifier.ACTION_SKIP -> Prefs.pushAction(context, "skip", id, at)
            Notifier.ACTION_SNOOZE -> Scheduler.snooze(context, ex, 10)
            else -> { /* «Выключить»: звук уже остановлен */ }
        }
    }
}

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Scheduler.restore(context)
        WidgetRenderer.updateAll(context)
    }
}
