use crate::model::Stats;

/// Показатель, по которому считается прогресс достижения.
#[derive(Debug, Clone, Copy)]
pub enum Metric {
    Done,
    OnTime,
    BestStreak,
    EarlyBird,
    NightOwl,
    Categories,
    Created,
    PerfectDays,
    Level,
}

impl Metric {
    pub fn value(&self, s: &Stats) -> u32 {
        match self {
            Metric::Done => s.total_done,
            Metric::OnTime => s.on_time,
            Metric::BestStreak => s.best_streak,
            Metric::EarlyBird => s.early_bird,
            Metric::NightOwl => s.night_owl,
            Metric::Categories => s.categories_used,
            Metric::Created => s.reminders_created,
            Metric::PerfectDays => s.perfect_days,
            Metric::Level => s.level,
        }
    }
}

pub struct Def {
    pub id: &'static str,
    pub icon: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    pub metric: Metric,
    pub target: u32,
    /// Очки рейтинга за получение.
    pub reward: i64,
}

pub const DEFS: &[Def] = &[
    Def { id: "first_step", icon: "🌱", title: "Первый шаг", desc: "Выполните первое напоминание", metric: Metric::Done, target: 1, reward: 5 },
    Def { id: "done_10", icon: "💪", title: "Втянулся", desc: "Выполните 10 напоминаний", metric: Metric::Done, target: 10, reward: 20 },
    Def { id: "done_50", icon: "🏅", title: "Полтинник", desc: "Выполните 50 напоминаний", metric: Metric::Done, target: 50, reward: 60 },
    Def { id: "done_100", icon: "💯", title: "Сотня", desc: "Выполните 100 напоминаний", metric: Metric::Done, target: 100, reward: 120 },
    Def { id: "done_500", icon: "🏆", title: "Легенда дел", desc: "Выполните 500 напоминаний", metric: Metric::Done, target: 500, reward: 400 },
    Def { id: "streak_3", icon: "🔥", title: "Три дня подряд", desc: "Серия из 3 дней без пропусков", metric: Metric::BestStreak, target: 3, reward: 15 },
    Def { id: "streak_7", icon: "🔥", title: "Неделя огня", desc: "Серия из 7 дней без пропусков", metric: Metric::BestStreak, target: 7, reward: 40 },
    Def { id: "streak_30", icon: "🌋", title: "Месяц дисциплины", desc: "Серия из 30 дней без пропусков", metric: Metric::BestStreak, target: 30, reward: 150 },
    Def { id: "streak_100", icon: "⚡", title: "Железная воля", desc: "Серия из 100 дней без пропусков", metric: Metric::BestStreak, target: 100, reward: 500 },
    Def { id: "punctual", icon: "⏱️", title: "Пунктуальный", desc: "25 напоминаний выполнено вовремя", metric: Metric::OnTime, target: 25, reward: 40 },
    Def { id: "early_bird", icon: "🐦", title: "Жаворонок", desc: "5 дел, назначенных до 8:00", metric: Metric::EarlyBird, target: 5, reward: 25 },
    Def { id: "night_owl", icon: "🦉", title: "Сова", desc: "5 дел, назначенных после 22:00", metric: Metric::NightOwl, target: 5, reward: 25 },
    Def { id: "organizer", icon: "🗂️", title: "Организатор", desc: "Используйте 3 разные категории", metric: Metric::Categories, target: 3, reward: 20 },
    Def { id: "planner", icon: "📝", title: "Планировщик", desc: "Создайте 10 напоминаний", metric: Metric::Created, target: 10, reward: 20 },
    Def { id: "perfect_days", icon: "✨", title: "Идеальные дни", desc: "7 дней без единого пропуска", metric: Metric::PerfectDays, target: 7, reward: 50 },
    Def { id: "level_5", icon: "🎖️", title: "Пятый уровень", desc: "Достигните 5 уровня", metric: Metric::Level, target: 5, reward: 30 },
];

/// Уровень по очкам: 1-й — до 50 очков, 2-й — до 200, 3-й — до 450 и т. д.
pub fn level_for(points: i64) -> (u32, f64) {
    let p = points.max(0) as f64;
    let level = (p / 50.0).sqrt().floor() as u32 + 1;
    let start = ((level - 1) as f64).powi(2) * 50.0;
    let next = (level as f64).powi(2) * 50.0;
    (level, (p - start) / (next - start))
}
