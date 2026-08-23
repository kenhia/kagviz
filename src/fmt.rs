//! Shared formatting for durations and counts.
//!
//! Presentation only — nothing here computes a fact, it just decides how one
//! reads. Both the terminal output and the HTML report go through it so a
//! duration never renders two different ways.

/// A duration as `42s`, `7m`, `3h05m`, or `54d01h`.
///
/// The day rung exists because resumed sessions are the normal case, not the
/// exotic one: the hardest session in the corpus spans 54 days, and rendering
/// that as `1297h15m` asks the reader to do the division. That number sits in
/// the headline precisely so wall clock stays legible next to active time —
/// unreadable would defeat the point of putting it there.
pub fn duration(secs: i64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = mins / 60;
    if hours < 24 {
        format!("{hours}h{:02}m", mins % 60)
    } else {
        format!("{}d{:02}h", hours / 24, hours % 24)
    }
}

/// A count with thousands separators, so six-digit token totals stay readable.
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_at_the_right_scale() {
        assert_eq!(duration(0), "0s");
        assert_eq!(duration(59), "59s");
        assert_eq!(duration(60), "1m");
        assert_eq!(duration(3599), "59m");
        assert_eq!(duration(3600), "1h00m");
        assert_eq!(duration(11100), "3h05m");
        assert_eq!(duration(86_399), "23h59m");
        assert_eq!(duration(86_400), "1d00h");
        // The corpus's worst case: 54 days, not 1297 hours.
        assert_eq!(duration(4_669_200), "54d01h");
    }

    #[test]
    fn counts_are_grouped_in_threes() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1000), "1,000");
        assert_eq!(count(1234567), "1,234,567");
    }
}
