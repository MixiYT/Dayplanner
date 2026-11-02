package app.dayplanner.nativebridge

import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView

/** Напоминание на весь экран: показывается поверх блокировки и включает экран. */
class AlertActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(
                WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or
                    WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON or
                    WindowManager.LayoutParams.FLAG_DISMISS_KEYGUARD
            )
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        val ex = intent.extras
        if (ex == null) {
            finish()
            return
        }
        val accent = try {
            Color.parseColor(Prefs.widget(this).optString("accent", "#6750A4"))
        } catch (e: Exception) {
            Color.parseColor("#6750A4")
        }
        val isAlarm = ex.getString("alert") == "alarm"

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(dp(28), dp(48), dp(28), dp(36))
            background = GradientDrawable(
                GradientDrawable.Orientation.TOP_BOTTOM,
                intArrayOf(darken(accent, 0.55f), darken(accent, 0.25f))
            )
        }
        fun text(s: String, sp: Float, bold: Boolean = false, alpha: Float = 1f) = TextView(this).apply {
            text = s
            textSize = sp
            setTextColor(Color.WHITE)
            this.alpha = alpha
            gravity = Gravity.CENTER
            if (bold) setTypeface(typeface, Typeface.BOLD)
        }

        val icon = ex.getString("icon") ?: "🔔"
        val bitmap = PlannerIcon.bitmap(icon)
        if (bitmap != null) root.addView(android.widget.ImageView(this).apply { setImageBitmap(bitmap) }, LinearLayout.LayoutParams(dp(96), dp(96)))
        else root.addView(text(PlannerIcon.text(icon), 84f))
        root.addView(text(ex.getString("title") ?: "", 28f, bold = true).apply { setPadding(0, dp(16), 0, dp(6)) })
        root.addView(text(TimeFmt.hhmm(ex.getString("at") ?: ""), 20f, alpha = 0.85f))
        val notes = ex.getString("notes") ?: ""
        if (notes.isNotBlank()) root.addView(text(notes, 16f, alpha = 0.8f).apply { setPadding(0, dp(12), 0, 0) })
        root.addView(View(this), LinearLayout.LayoutParams(0, 0, 1f).apply { height = dp(40) })

        fun button(label: String, primary: Boolean, action: String, snooze: Boolean = false) {
            val b = Button(this).apply {
                text = label
                isAllCaps = false
                textSize = 17f
                setTextColor(if (primary) accent else Color.WHITE)
                background = GradientDrawable().apply {
                    cornerRadius = dp(28).toFloat()
                    if (primary) setColor(Color.WHITE) else setStroke(dp(2), Color.argb(150, 255, 255, 255))
                }
                setOnClickListener {
                    sendBroadcast(
                        Intent(this@AlertActivity, ActionReceiver::class.java).setAction(action)
                            .putExtras(ex)
                    )
                    finish()
                }
            }
            root.addView(b, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(56)).apply { topMargin = dp(12) })
        }
        button("Выполнено", true, Notifier.ACTION_DONE)
        if (isAlarm) button("Выключить", false, Notifier.ACTION_STOP)
        button("Через 10 минут", false, Notifier.ACTION_SNOOZE)
        button("Пропустить", false, Notifier.ACTION_SKIP)

        setContentView(root)
    }

    private fun dp(v: Int) = (v * resources.displayMetrics.density).toInt()

    private fun darken(color: Int, f: Float): Int =
        Color.rgb((Color.red(color) * f).toInt(), (Color.green(color) * f).toInt(), (Color.blue(color) * f).toInt())
}
