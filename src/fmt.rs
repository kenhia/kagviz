//! Shared formatting for durations and counts.
//!
//! Presentation only — nothing here computes a fact, it just decides how one
//! reads. Both the terminal output and the HTML report go through it so a
//! duration never renders two different ways.

/// A duration as `42s`, `7m`, or `3h05m`.
pub fn duration(secs: i64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        format!("{mins}m")
    } else {
        format!("{}h{:02}m", mins / 60, mins % 60)
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
    }

    #[test]
    fn counts_are_grouped_in_threes() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1000), "1,000");
        assert_eq!(count(1234567), "1,234,567");
    }
}
