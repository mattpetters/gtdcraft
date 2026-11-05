//! Typed dates and durations the way people write them (date fields accept natural text):
//! "today", "tomorrow 5pm", "fri", "next week", "+3d", "2w", "+3h", "in 3 days", "nov 15", "11/15 9:30", "2026-11-15 17:00".
//! Rules (see MECHANICS.md "Typed dates"): modifier next = -1, last = +1,
//! none = 0; unit codes hour 0, day 1, week 2 (x7 days), month 3, year 4.
use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, NaiveTime, Weekday};

/// Ok(None) = cleared (empty, "none", "clear"); Err = couldn't read it.
pub fn parse_when(input: &str, now: NaiveDateTime, default_time: NaiveTime) -> Result<Option<NaiveDateTime>, String> {
    let s = input.trim().to_lowercase();
    if s.is_empty() || s == "none" || s == "clear" || s == "-" {
        return Ok(None);
    }
    // exact formats first
    for f in ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(d) = NaiveDateTime::parse_from_str(&s, f) {
            return Ok(Some(d));
        }
    }
    let today = now.date();
    let words: Vec<&str> = s.split_whitespace().collect();
    let mut date: Option<NaiveDate> = None;
    let mut time: Option<NaiveTime> = None;
    let mut i = 0;
    while i < words.len() {
        let w = words[i].trim_matches(',');
        let next = words.get(i + 1).map(|x| x.trim_matches(','));
        if let Some(t) = parse_time(w, next) {
            time = Some(t.0);
            i += t.1;
            continue;
        }
        // "now", "tonight" (special-case time names) and hour offsets ("+3h", "3 hours", "in 3 hours") set the time too
        if let Some((dt, used)) = parse_moment(w, next, words.get(i + 2).copied(), now) {
            date = Some(dt.date());
            time = Some(dt.time());
            i += used;
            continue;
        }
        if let Some(d) = parse_day(w, next, words.get(i + 2).copied(), today) {
            date = Some(d.0);
            i += d.1;
            continue;
        }
        return Err(format!("Couldn't read “{}”. Try today, tomorrow 5pm, fri, +3d, nov 15 or 2026-11-15 17:00.", input.trim()));
    }
    let date = date.unwrap_or(today);
    Ok(Some(date.and_time(time.unwrap_or(default_time))))
}

fn weekday(w: &str) -> Option<Weekday> {
    let names = [("monday", Weekday::Mon), ("tuesday", Weekday::Tue), ("wednesday", Weekday::Wed), ("thursday", Weekday::Thu), ("friday", Weekday::Fri), ("saturday", Weekday::Sat), ("sunday", Weekday::Sun)];
    names.iter().find(|(n, _)| w.len() >= 3 && n.starts_with(w)).map(|x| x.1)
}

fn month(w: &str) -> Option<u32> {
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|m| w.starts_with(m) && w.len() <= 9).map(|i| i as u32 + 1)
}

/// "now" = this moment; "tonight" = today 23:00; hours add to now (unit code 0).
fn parse_moment(w: &str, next: Option<&str>, next2: Option<&str>, now: NaiveDateTime) -> Option<(NaiveDateTime, usize)> {
    let hour = |u: &str| matches!(u, "h" | "hr" | "hrs" | "hour" | "hours");
    match w {
        "now" => return Some((now, 1)),
        "tonight" => return Some((now.date().and_hms_opt(23, 0, 0)?, 1)),
        "in" if hour(next2?) => return Some((now + Duration::hours(next?.parse().ok()?), 3)),
        _ => {}
    }
    let t = w.trim_start_matches('+');
    let at = t.find(|c: char| c.is_ascii_alphabetic());
    if let Some(at) = at {
        if hour(&t[at..]) {
            return Some((now + Duration::hours(t[..at].parse().ok()?), 1));
        }
    }
    if let (Ok(n), Some(u)) = (t.parse::<i64>(), next) {
        if hour(u) {
            return Some((now + Duration::hours(n), 2));
        }
    }
    None
}

/// Days from today to a weekday: no modifier = the
/// next one but today stays today; next (-1) = target - current + 7; last (+1) = target - current - 7.
fn weekday_offset(wd: Weekday, today: NaiveDate, modifier: i32) -> i64 {
    let diff = wd.num_days_from_sunday() as i64 - today.weekday().num_days_from_sunday() as i64;
    match modifier {
        -1 => diff + 7,
        1 => diff - 7,
        _ => if diff < 0 { diff + 7 } else { diff },
    }
}

/// Relative amount like "3d", "+2w", "1m", "1y" (and "3" + "days").
fn offset(n: i64, unit: &str, from: NaiveDate) -> Option<NaiveDate> {
    let u = unit.trim_end_matches('s');
    match u {
        "d" | "day" => Some(from + Duration::days(n)),
        "w" | "wk" | "week" => Some(from + Duration::weeks(n)),
        "m" | "mo" | "month" => if n >= 0 { from.checked_add_months(Months::new(n as u32)) } else { from.checked_sub_months(Months::new((-n) as u32)) },
        "y" | "yr" | "year" => if n >= 0 { from.checked_add_months(Months::new(12 * n as u32)) } else { from.checked_sub_months(Months::new(12 * (-n) as u32)) },
        _ => None,
    }
}

/// Returns (date, words used).
fn parse_day(w: &str, next: Option<&str>, next2: Option<&str>, today: NaiveDate) -> Option<(NaiveDate, usize)> {
    match w {
        "today" | "tod" => return Some((today, 1)),
        "tomorrow" | "tom" | "tmrw" => return Some((today + Duration::days(1), 1)),
        "yesterday" | "yes" => return Some((today - Duration::days(1), 1)),
        // multiplier: last = -1, otherwise +1
        "next" | "last" | "this" => {
            let n = next?;
            let modifier = match w { "next" => -1, "last" => 1, _ => 0 };
            if let Some(wd) = weekday(n) {
                return Some((today + Duration::days(weekday_offset(wd, today, modifier)), 2));
            }
            if let Some(m) = month(n) {
                return month_year(today, m, modifier).and_then(|y| NaiveDate::from_ymd_opt(y, m, 1)).map(|d| (d, 2));
            }
            return offset(-(modifier as i64), n, today).map(|d| (d, 2));
        }
        "in" => {
            let n: i64 = next?.parse().ok()?;
            return offset(n, next2?, today).map(|d| (d, 3));
        }
        _ => {}
    }
    if let Some(wd) = weekday(w) {
        return Some((today + Duration::days(weekday_offset(wd, today, 0)), 1));
    }
    // +3d, 3d, -1w
    let t = w.trim_start_matches('+');
    let split = t.find(|c: char| c.is_ascii_alphabetic());
    if let Some(at) = split {
        if let Ok(n) = t[..at].parse::<i64>() {
            if let Some(d) = offset(n, &t[at..], today) {
                return Some((d, 1));
            }
        }
    }
    // "3 days"
    if let (Ok(n), Some(u)) = (t.parse::<i64>(), next) {
        if let Some(d) = offset(n, u, today) {
            return Some((d, 2));
        }
    }
    // nov 15 / 15 nov
    if let Some(m) = month(w) {
        let day: u32 = next?.trim_end_matches(|c: char| c.is_ascii_alphabetic()).parse().ok()?;
        return future_md(today, m, day).map(|d| (d, 2));
    }
    if let (Ok(day), Some(m)) = (w.parse::<u32>(), next.and_then(month)) {
        return future_md(today, m, day).map(|d| (d, 2));
    }
    // 11/15 or 11/15/26 or 2026-11-15
    if let Ok(d) = NaiveDate::parse_from_str(w, "%Y-%m-%d") {
        return Some((d, 1));
    }
    let parts: Vec<&str> = w.split('/').collect();
    if parts.len() == 2 || parts.len() == 3 {
        let m: u32 = parts[0].parse().ok()?;
        let day: u32 = parts[1].parse().ok()?;
        if parts.len() == 3 {
            let mut y: i32 = parts[2].parse().ok()?;
            if y < 100 {
                y += 2000;
            }
            return NaiveDate::from_ymd_opt(y, m, day).map(|d| (d, 1));
        }
        return future_md(today, m, day).map(|d| (d, 1));
    }
    None
}

/// Year for a month without one:
/// compares months only. Same month: this year (next = +1, last = -1). Later month: this year (last = -1).
/// Earlier month: next year (last = this year).
fn month_year(today: NaiveDate, m: u32, modifier: i32) -> Option<i32> {
    let (cur, y) = (today.month(), today.year());
    Some(if cur == m {
        match modifier { 1 => y - 1, -1 => y + 1, _ => y }
    } else if cur < m {
        if modifier == 1 { y - 1 } else { y }
    } else if modifier == 1 { y } else { y + 1 })
}

/// Month/day without a year.
fn future_md(today: NaiveDate, m: u32, day: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(month_year(today, m, 0)?, m, day)
}

/// Returns (time, words used): "5pm", "5 pm", "17:00", "9:30am", "noon", "midnight", "at 5pm".
fn parse_time(w: &str, next: Option<&str>) -> Option<(NaiveTime, usize)> {
    match w {
        "noon" => return Some((NaiveTime::from_hms_opt(12, 0, 0)?, 1)),
        "midnight" => return Some((NaiveTime::MIN, 1)),
        "at" => return parse_time(next?, None).map(|(t, _)| (t, 2)),
        _ => {}
    }
    let (body, suffix, used) = if let Some(b) = w.strip_suffix("am").or_else(|| w.strip_suffix('a')) {
        (b, Some(false), 1)
    } else if let Some(b) = w.strip_suffix("pm").or_else(|| w.strip_suffix('p')) {
        (b, Some(true), 1)
    } else if matches!(next, Some("am")) {
        (w, Some(false), 2)
    } else if matches!(next, Some("pm")) {
        (w, Some(true), 2)
    } else {
        (w, None, 1)
    };
    if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return None;
    }
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if suffix.is_some() => (body.parse::<u32>().ok()?, 0),
        None => return None, // a bare number is a day, not a time
    };
    let h = match suffix {
        Some(true) if h < 12 => h + 12,
        Some(false) if h == 12 => 0,
        _ => h,
    };
    NaiveTime::from_hms_opt(h, m, 0).map(|t| (t, used))
}

/// Minutes from "15", "15m", "1h", "1h30m", "1:30", "90 min".
pub fn parse_minutes(input: &str) -> Result<Option<u32>, String> {
    let s = input.trim().to_lowercase().replace(' ', "");
    if s.is_empty() || s == "none" {
        return Ok(None);
    }
    if let Some((h, m)) = s.split_once(':') {
        if let (Ok(h), Ok(m)) = (h.parse::<u32>(), m.parse::<u32>()) {
            return Ok(Some(h * 60 + m));
        }
    }
    let mut total = 0u32;
    let mut num = String::new();
    let mut any = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: u32 = num.parse().map_err(|_| format!("Couldn't read “{}”", input.trim()))?;
        num.clear();
        let mut unit = c.to_string();
        while let Some(&n2) = chars.peek() {
            if n2.is_ascii_alphabetic() {
                unit.push(n2);
                chars.next();
            } else {
                break;
            }
        }
        match unit.as_str() {
            "h" | "hr" | "hrs" | "hour" | "hours" => total += n * 60,
            "m" | "min" | "mins" | "minute" | "minutes" => total += n,
            _ => return Err(format!("Couldn't read “{}”. Try 15m, 1h or 1h30m.", input.trim())),
        }
        any = true;
    }
    if !num.is_empty() {
        total += num.parse::<u32>().map_err(|_| "bad number".to_string())?;
        any = true;
    }
    if any { Ok(Some(total)) } else { Err(format!("Couldn't read “{}”", input.trim())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn natural_dates() {
        let now = at(2, 12); // Monday, Nov 2 2026
        let five = NaiveTime::from_hms_opt(17, 0, 0).unwrap();
        let p = |s: &str| parse_when(s, now, five).unwrap().unwrap();
        assert_eq!(p("today"), at(2, 17));
        assert_eq!(p("tomorrow 9am"), at(3, 9));
        assert_eq!(p("tom at 5pm"), at(3, 17));
        assert_eq!(p("fri"), at(6, 17));
        assert_eq!(p("mon"), at(2, 17)); // same weekday stays today
        assert_eq!(p("sun"), at(8, 17));
        assert_eq!(p("next fri"), at(13, 17)); // target - current + 7
        assert_eq!(p("next mon"), at(9, 17));
        assert_eq!(p("last fri"), NaiveDate::from_ymd_opt(2026, 10, 30).unwrap().and_time(five));
        assert_eq!(p("last week"), NaiveDate::from_ymd_opt(2026, 10, 26).unwrap().and_time(five));
        assert_eq!(p("next month"), NaiveDate::from_ymd_opt(2026, 12, 2).unwrap().and_time(five));
        assert_eq!(p("this month"), at(2, 17));
        assert_eq!(p("yes"), NaiveDate::from_ymd_opt(2026, 11, 1).unwrap().and_time(five));
        assert_eq!(p("tonight"), at(2, 23));
        assert_eq!(p("now"), at(2, 12));
        assert_eq!(p("+3h"), at(2, 15));
        assert_eq!(p("in 2 hours"), at(2, 14));
        assert_eq!(p("nov 1"), at(1, 17)); // same month keeps this year even if the day passed
        assert_eq!(p("oct 5"), NaiveDate::from_ymd_opt(2027, 10, 5).unwrap().and_time(five));
        assert_eq!(p("+3d"), at(5, 17));
        assert_eq!(p("2w"), at(16, 17));
        assert_eq!(p("in 3 days"), at(5, 17));
        assert_eq!(p("next week"), at(9, 17));
        assert_eq!(p("nov 15 9:30am"), NaiveDate::from_ymd_opt(2026, 11, 15).unwrap().and_hms_opt(9, 30, 0).unwrap());
        assert_eq!(p("15 nov"), at(15, 17));
        assert_eq!(p("11/20"), at(20, 17));
        assert_eq!(p("1/5"), NaiveDate::from_ymd_opt(2027, 1, 5).unwrap().and_time(five)); // past month/day rolls to next year
        assert_eq!(p("2026-11-15 08:00"), at(15, 8));
        assert_eq!(p("2026-11-15"), at(15, 17));
        assert_eq!(p("noon"), at(2, 12));
        assert_eq!(p("12am"), at(2, 0));
        assert_eq!(parse_when("", now, five).unwrap(), None);
        assert!(parse_when("whenever", now, five).is_err());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_minutes("15").unwrap(), Some(15));
        assert_eq!(parse_minutes("15m").unwrap(), Some(15));
        assert_eq!(parse_minutes("1h30m").unwrap(), Some(90));
        assert_eq!(parse_minutes("1:30").unwrap(), Some(90));
        assert_eq!(parse_minutes("2 hours").unwrap(), Some(120));
        assert_eq!(parse_minutes("").unwrap(), None);
        assert!(parse_minutes("lots").is_err());
    }
}
