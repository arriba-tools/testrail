use anyhow::Result;
use chrono::{Local, TimeZone};
use serde::Serialize;
use serde_json::Value;

/// Print a value as pretty JSON on stdout.
pub fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Align rows into columns separated by two spaces. The last column is left unpadded.
pub fn table(rows: &[Vec<String>], indent: &str) -> String {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..cols)
        .map(|c| {
            rows.iter()
                .filter_map(|r| r.get(c))
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    if i + 1 == row.len() {
                        cell.clone()
                    } else {
                        let pad = widths[i] - cell.chars().count();
                        format!("{cell}{}", " ".repeat(pad))
                    }
                })
                .collect();
            format!("{indent}{}", cells.join("  ").trim_end())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Print rows as a table, or `empty` when there are none.
pub fn print_table(rows: &[Vec<String>], empty: &str) {
    if rows.is_empty() {
        println!("{empty}");
    } else {
        println!("{}", table(rows, ""));
    }
}

/// Note on stderr that a list stopped at its limit.
pub fn note_more(more: bool, shown: usize) {
    if more {
        eprintln!("Showing the first {shown}. Pass --limit or --all for more.");
    }
}

/// Byte count with a binary unit, as in `512 B` or `1.5 MB`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// A TestRail timestamp, in Unix seconds, as local time without seconds. Null reads as empty.
pub fn date(v: &Value) -> String {
    v.as_i64()
        .and_then(|t| Local.timestamp_opt(t, 0).single())
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// A day from a TestRail timestamp, for due and start dates.
pub fn day(v: &Value) -> String {
    v.as_i64()
        .and_then(|t| Local.timestamp_opt(t, 0).single())
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// A string field, or empty.
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// `1 case` or `3 cases`.
pub fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The first line of a text, cut to `max` characters.
pub fn first_line(text: &str, max: usize) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() > max {
        let cut: String = line.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

/// A run or plan's tallies, as in `12 passed, 1 failed, 27 untested`, from its `*_count` fields.
pub fn counts(v: &Value) -> String {
    let get = |k: &str| v[k].as_u64().unwrap_or(0);
    let mut parts: Vec<String> = [
        ("passed", "passed_count"),
        ("blocked", "blocked_count"),
        ("retest", "retest_count"),
        ("failed", "failed_count"),
        ("untested", "untested_count"),
    ]
    .iter()
    .filter(|(_, k)| get(k) > 0)
    .map(|(name, k)| format!("{} {name}", get(k)))
    .collect();
    let custom: u64 = (1..=7)
        .map(|i| get(&format!("custom_status{i}_count")))
        .sum();
    if custom > 0 {
        parts.push(format!("{custom} other"));
    }
    if parts.is_empty() {
        "no tests".to_string()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn table_aligns_columns() {
        let rows = vec![
            vec!["C3".to_string(), "a".to_string(), "x".to_string()],
            vec!["C12".to_string(), "bbb".to_string(), "y".to_string()],
        ];
        assert_eq!(table(&rows, "  "), "  C3   a    x\n  C12  bbb  y");
    }

    #[test]
    fn sizes_use_binary_units() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1536), "1.5 KB");
        assert_eq!(size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn timestamps_read_as_local_dates() {
        assert_eq!(date(&json!(null)), "");
        assert_eq!(date(&json!(1_700_000_000)).len(), 16);
        assert_eq!(day(&json!(1_700_000_000)).len(), 10);
    }

    #[test]
    fn text_helpers() {
        assert_eq!(text(&json!("a")), "a");
        assert_eq!(text(&json!(3)), "3");
        assert_eq!(text(&json!(null)), "");
        assert_eq!(plural(1, "case"), "1 case");
        assert_eq!(plural(2, "case"), "2 cases");
        assert_eq!(first_line("\n  hello there\nmore", 5), "hell…");
        assert_eq!(first_line("hi", 5), "hi");
    }

    #[test]
    fn counts_list_nonzero_tallies() {
        let run = json!({
            "passed_count": 12, "failed_count": 1, "untested_count": 27,
            "blocked_count": 0, "custom_status2_count": 3
        });
        assert_eq!(counts(&run), "12 passed, 1 failed, 27 untested, 3 other");
        assert_eq!(counts(&json!({})), "no tests");
    }
}
