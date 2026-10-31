//! Даты без внешних библиотек: секунды Unix → год, месяц, день.
//! Алгоритм Говарда Хиннанта «days_from_civil» наоборот — точен для любых дат.

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn from_unix(secs: i64) -> Civil {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    Civil { year, month, day, hour: rem / 3600, minute: rem % 3600 / 60, second: rem % 60 }
}

pub fn unix(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

pub fn now() -> i64 {
    unix(SystemTime::now())
}

/// `2026-10` — имя папки месяца.
pub fn month_folder(secs: i64) -> String {
    let c = from_unix(secs);
    format!("{}-{:02}", c.year, c.month)
}

/// `2026-10-31_23-59-07` — сортируется как строка.
pub fn stamp(secs: i64) -> String {
    let c = from_unix(secs);
    format!("{}-{:02}-{:02}_{:02}-{:02}-{:02}", c.year, c.month, c.day, c.hour, c.minute, c.second)
}

/// `26.10.2026` — для имён папок.
pub fn date_ru(secs: i64) -> String {
    let c = from_unix(secs);
    format!("{:02}.{:02}.{}", c.day, c.month, c.year)
}
