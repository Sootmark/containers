//! The times cloud logs write: ISO 8601 with `T` or a space, a fraction of
//! any length, `Z`, an offset, or no zone (UTC, as Microsoft 365 writes
//! `CreationTime`).

use common::time::{days_from_civil, Precision, Ts};

const TICKS_PER_SECOND: i64 = 10_000_000;

/// A time as UTC.
pub(crate) fn parse(text: &str) -> Option<Ts> {
    let text = text.trim();
    let (date, clock) = text.split_once(['T', ' '])?;
    let mut date = date.splitn(3, '-');
    let year: i64 = date
        .next()?
        .parse()
        .ok()
        .filter(|y| (1..=9999).contains(y))?;
    let month: u32 = date.next()?.parse().ok().filter(|m| (1..=12).contains(m))?;
    let day: u32 = date.next()?.parse().ok().filter(|d| (1..=31).contains(d))?;
    let (clock, zone) = match clock.find(['Z', 'z', '+', '-']) {
        Some(at) => (&clock[..at], &clock[at..]),
        None => (clock, ""),
    };
    let (whole, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut parts = whole.splitn(3, ':');
    let hour: i64 = parts.next()?.parse().ok().filter(|h| (0..24).contains(h))?;
    let minute: i64 = parts.next()?.parse().ok().filter(|m| (0..60).contains(m))?;
    let second: i64 = parts
        .next()
        .unwrap_or("0")
        .parse()
        .ok()
        .filter(|s| (0..=60).contains(s))?;
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits: String = fraction.chars().take(7).collect();
    let sub: i64 = if digits.is_empty() {
        0
    } else {
        format!("{digits:0<7}").parse().ok()?
    };
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    let precision = if fraction.is_empty() {
        Precision::Second
    } else {
        Precision::Tick
    };
    let local = Ts::from_local_ticks(seconds * TICKS_PER_SECOND + sub, precision);
    Some(local.assume_offset(offset_minutes(zone)?))
}

/// `Z`, `+11:00`, `-0500`, or nothing (UTC) in minutes.
fn offset_minutes(zone: &str) -> Option<i32> {
    if zone.is_empty() || zone.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let digits: String = zone[1..].chars().filter(|c| *c != ':').collect();
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = digits[..2].parse().ok()?;
    let minutes: i32 = digits[2..].parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings() {
        let iso = |t: &str| parse(t).and_then(|t| t.to_iso8601());
        assert_eq!(
            iso("2022-05-07T12:55:53").as_deref(),
            Some("2022-05-07T12:55:53.0000000Z")
        );
        assert_eq!(
            iso("2022-02-08 09:23:37+11:00").as_deref(),
            Some("2022-02-07T22:23:37.0000000Z")
        );
        assert_eq!(
            iso("2021-10-19T02:57:47.834659236Z").as_deref(),
            Some("2021-10-19T02:57:47.8346592Z")
        );
        assert_eq!(
            iso("2021-10-18T19:57:39.584-0700").as_deref(),
            Some("2021-10-19T02:57:39.5840000Z")
        );
        assert_eq!(iso("yesterday"), None);
        assert_eq!(iso("2021-10-18T19:57:39+7"), None);
    }
}
