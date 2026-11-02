use chrono::{Datelike, NaiveDate, NaiveDateTime};

use crate::model::{Reminder, Repeat};

pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

fn matches(r: &Reminder, d: NaiveDate) -> bool {
    let sd = r.start.date();
    if d < sd {
        return false;
    }
    match &r.repeat {
        Repeat::None => d == sd,
        Repeat::Daily => true,
        Repeat::Weekdays { days } => days.contains(&(d.weekday().number_from_monday() as u8)),
        // «31-го» в коротком месяце переносится на последний день месяца.
        Repeat::Monthly => d.day() == sd.day().min(days_in_month(d.year(), d.month())),
    }
}

/// Все срабатывания напоминания в диапазоне дат (обе границы включительно).
pub fn occurrences(r: &Reminder, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDateTime> {
    let mut out = Vec::new();
    let first = from.max(r.start.date());
    let last = match r.repeat_until {
        Some(u) => to.min(u),
        None => to,
    };
    let mut d = first;
    let mut guard = 0;
    while d <= last && guard < 4000 {
        if matches(r, d) {
            out.push(d.and_time(r.start.time()));
        }
        match d.succ_opt() {
            Some(n) => d = n,
            None => break,
        }
        guard += 1;
    }
    out
}
