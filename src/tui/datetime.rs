//! Local-timezone date helpers shared by operation records and statistics.
use time::{Date, Duration as TimeDuration, Month, OffsetDateTime, UtcOffset};

fn local_utc_offset() -> UtcOffset {
    UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)
}

pub(in crate::tui) fn today_local_date() -> Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

pub(in crate::tui) fn timestamp_to_local_date(timestamp: i64) -> Option<Date> {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()
        .map(|timestamp| timestamp.to_offset(local_utc_offset()).date())
}

pub(in crate::tui) fn date_start_timestamp(date: Date) -> i64 {
    date.midnight()
        .assume_offset(local_utc_offset())
        .unix_timestamp()
}

pub(in crate::tui) fn date_end_timestamp(date: Date) -> i64 {
    date_start_timestamp(date.saturating_add(TimeDuration::DAY)).saturating_sub(1)
}
fn month_from_number(month: u8) -> Month {
    match month {
        1 => Month::January,
        2 => Month::February,
        3 => Month::March,
        4 => Month::April,
        5 => Month::May,
        6 => Month::June,
        7 => Month::July,
        8 => Month::August,
        9 => Month::September,
        10 => Month::October,
        11 => Month::November,
        _ => Month::December,
    }
}

pub(in crate::tui) fn add_months_to_date(date: Date, delta: i32) -> Date {
    let month_number = i32::from(date.month() as u8);
    let total = date
        .year()
        .saturating_mul(12)
        .saturating_add(month_number - 1)
        .saturating_add(delta);
    let year = total.div_euclid(12);
    let month = month_from_number((total.rem_euclid(12) + 1) as u8);
    let day = date.day().min(month.length(year));
    Date::from_calendar_date(year, month, day).unwrap_or(date)
}
