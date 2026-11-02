package app.dayplanner.nativebridge

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.widget.RemoteViews
import org.json.JSONArray
import org.json.JSONObject

class TodayWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, mgr: AppWidgetManager, ids: IntArray) =
        WidgetRenderer.update(context, mgr, ids, "today")
}

class ScheduleWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, mgr: AppWidgetManager, ids: IntArray) =
        WidgetRenderer.update(context, mgr, ids, "schedule")
}

class WeekWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, mgr: AppWidgetManager, ids: IntArray) =
        WidgetRenderer.update(context, mgr, ids, "week")
}

/** Рисует виджеты по данным, которые ядро прислало в `sync` (снимок на 7 дней вперёд). */
object WidgetRenderer {
    private val ROWS = intArrayOf(
        R.id.pn_r0, R.id.pn_r1, R.id.pn_r2, R.id.pn_r3, R.id.pn_r4, R.id.pn_r5, R.id.pn_r6, R.id.pn_r7
    )
    private val WD = arrayOf("Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс")
    private val MONTHS = arrayOf("янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек")

    fun updateAll(c: Context) {
        val mgr = AppWidgetManager.getInstance(c)
        listOf(
            TodayWidget::class.java to "today",
            ScheduleWidget::class.java to "schedule",
            WeekWidget::class.java to "week",
        ).forEach { (cls, kind) ->
            val ids = mgr.getAppWidgetIds(ComponentName(c, cls))
            if (ids.isNotEmpty()) update(c, mgr, ids, kind)
        }
    }

    fun update(c: Context, mgr: AppWidgetManager, ids: IntArray, kind: String) {
        val views = build(c, kind)
        ids.forEach { mgr.updateAppWidget(it, views) }
    }

    private class Palette(val bg: Int, val bgAlpha: Int, val text: Int, val muted: Int, val accent: Int)

    private fun blend(base: Int, over: Int, f: Float): Int = Color.rgb(
        (Color.red(base) * (1 - f) + Color.red(over) * f).toInt(),
        (Color.green(base) * (1 - f) + Color.green(over) * f).toInt(),
        (Color.blue(base) * (1 - f) + Color.blue(over) * f).toInt(),
    )

    private fun palette(c: Context, snap: JSONObject): Palette {
        val accent = try { Color.parseColor(snap.optString("accent", "#6750A4")) } catch (e: Exception) { Color.parseColor("#6750A4") }
        val theme = snap.optString("theme", "material")
        val dark = theme == "amoled" || when (snap.optString("mode", "auto")) {
            "dark" -> true
            "light" -> false
            else -> (c.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        }
        val text = if (dark) Color.parseColor("#F1F1F3") else Color.parseColor("#1C1B1F")
        val muted = if (dark) Color.parseColor("#A19CA8") else Color.parseColor("#6B6872")
        // Яркий акцент в тёмной теме для заголовка
        val head = if (dark) blend(accent, Color.WHITE, 0.35f) else blend(accent, Color.BLACK, 0.15f)
        return when (theme) {
            "amoled" -> Palette(Color.BLACK, 255, text, muted, head)
            "glass" -> if (dark) Palette(Color.WHITE, 60, Color.WHITE, Color.parseColor("#CCFFFFFF"), head)
            else Palette(Color.WHITE, 170, text, muted, head)
            "simple" -> Palette(if (dark) Color.parseColor("#202024") else Color.WHITE, 255, text, muted, head)
            else -> Palette(blend(if (dark) Color.parseColor("#141218") else Color.parseColor("#FDFBFF"), accent, if (dark) 0.16f else 0.1f), 255, text, muted, head)
        }
    }

    private fun dayLabel(date: String): String {
        val p = date.split("-")
        if (p.size != 3) return date
        val cal = java.util.Calendar.getInstance().apply { set(p[0].toInt(), p[1].toInt() - 1, p[2].toInt()) }
        val wd = (cal.get(java.util.Calendar.DAY_OF_WEEK) + 5) % 7
        return "${WD[wd]} ${p[2].toInt()} ${MONTHS[p[1].toInt() - 1]}"
    }

    private fun plural(n: Int, one: String, few: String, many: String): String {
        val m = n % 100
        val k = n % 10
        return "$n " + if (m in 11..19) many else if (k == 1) one else if (k in 2..4) few else many
    }

    fun build(c: Context, kind: String): RemoteViews {
        val v = RemoteViews(c.packageName, R.layout.pn_widget)
        val snap = Prefs.widget(c)
        val pal = palette(c, snap)

        v.setInt(R.id.pn_bg, "setColorFilter", pal.bg)
        v.setInt(R.id.pn_bg, "setImageAlpha", pal.bgAlpha)
        v.setTextColor(R.id.pn_title, pal.accent)
        v.setTextColor(R.id.pn_sub, pal.muted)

        val lines = ArrayList<Triple<String, Int, Boolean>>() // текст, цвет, приглушить
        val days: JSONArray = snap.optJSONArray("days") ?: JSONArray()
        val today = TimeFmt.today()
        var title = "Ежедневник"
        var sub = ""

        fun dayAt(i: Int): JSONObject? = days.optJSONObject(i)
        val todayObj = (0 until days.length()).mapNotNull { dayAt(it) }.firstOrNull { it.optString("date") == today }

        if (todayObj == null) {
            title = "Ежедневник"
            sub = "Откройте приложение, чтобы обновить данные"
        } else when (kind) {
            "today" -> {
                val items = todayObj.optJSONArray("items") ?: JSONArray()
                var done = 0
                for (i in 0 until items.length()) if (items.getJSONObject(i).optBoolean("done")) done++
                title = "Дела · " + dayLabel(today)
                sub = if (items.length() == 0) "На сегодня дел нет" else "Выполнено $done из ${items.length()}" +
                    (if (snap.optInt("streak") > 0) " · 🔥 ${snap.optInt("streak")}" else "")
                for (i in 0 until items.length()) {
                    val it = items.getJSONObject(i)
                    val d = it.optBoolean("done")
                    val mark = if (d) "✅" else "⬜"
                    lines.add(Triple("$mark ${it.optString("time")}  ${PlannerIcon.text(it.optString("icon"))} ${it.optString("title")}", if (!d && it.optBoolean("overdue")) Color.parseColor("#D64545") else pal.text, d))
                }
            }
            "schedule" -> {
                val slots = todayObj.optJSONArray("schedule") ?: JSONArray()
                title = "Расписание · " + dayLabel(today)
                sub = if (slots.length() == 0) "Сегодня занятий нет" else plural(slots.length(), "занятие", "занятия", "занятий")
                for (i in 0 until slots.length()) {
                    val s = slots.getJSONObject(i)
                    val place = s.optString("location")
                    lines.add(Triple("${s.optString("start")}–${s.optString("end")}  ${PlannerIcon.text(s.optString("icon"))} ${s.optString("title")}" + (if (place.isNotBlank()) " · $place" else ""), pal.text, false))
                }
            }
            else -> {
                title = "Неделя"
                sub = "Занятия и дела на 7 дней"
                for (i in 0 until days.length()) {
                    val d = dayAt(i) ?: continue
                    val items = d.optJSONArray("items") ?: JSONArray()
                    val slots = d.optJSONArray("schedule") ?: JSONArray()
                    var done = 0
                    for (k in 0 until items.length()) if (items.getJSONObject(k).optBoolean("done")) done++
                    val parts = ArrayList<String>()
                    if (slots.length() > 0) parts.add(plural(slots.length(), "занятие", "занятия", "занятий"))
                    if (items.length() > 0) parts.add("дела $done/${items.length()}")
                    lines.add(Triple(dayLabel(d.optString("date")) + "   " + (if (parts.isEmpty()) "свободно" else parts.joinToString(" · ")), if (d.optString("date") == today) pal.accent else pal.text, false))
                }
            }
        }

        v.setTextViewText(R.id.pn_title, title)
        v.setTextViewText(R.id.pn_sub, sub)
        for ((idx, rid) in ROWS.withIndex()) {
            if (idx < lines.size) {
                val (text, color, dim) = lines[idx]
                v.setViewVisibility(rid, android.view.View.VISIBLE)
                v.setTextViewText(rid, text)
                v.setTextColor(rid, if (dim) pal.muted else color)
            } else {
                v.setViewVisibility(rid, android.view.View.GONE)
            }
        }

        // Нажатие на виджет открывает приложение.
        val launch = c.packageManager.getLaunchIntentForPackage(c.packageName)
        if (launch != null) {
            val pi = PendingIntent.getActivity(c, 10, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            v.setOnClickPendingIntent(R.id.pn_root, pi)
        }
        return v
    }
}
