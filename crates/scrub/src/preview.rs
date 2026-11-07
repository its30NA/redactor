//! The `--preview` report: one line per redaction, with the surrounding line and every secret on
//! it masked as `[label]`.

use crate::audit::Row;

const WIDTH: usize = 72;

/// Up to `width` characters of `text` around the character index `focus`.
fn window(text: &str, focus: usize, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    let half = width / 2;
    let mut start = focus.saturating_sub(half);
    let end = (start + width).min(chars.len());
    start = end.saturating_sub(width);
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < chars.len() { "…" } else { "" };
    let body: String = chars[start..end].iter().collect();
    format!("{prefix}{body}{suffix}")
}

/// Builds the report for `rows` (sorted by start, byte offsets into `text`).
pub fn preview(text: &str, rows: &[Row]) -> String {
    if rows.is_empty() {
        return "No sensitive data detected.".to_string();
    }
    let label_col = rows
        .iter()
        .map(|r| r.label.chars().count())
        .max()
        .unwrap_or(0)
        .min(28);
    let mut lines = vec![format!("Preview — {} redaction(s):", rows.len())];
    for row in rows {
        let line_start = text[..row.start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[row.end..]
            .find('\n')
            .map_or(text.len(), |i| row.end + i);
        // Mask every finding that touches this line, not only the one being listed.
        let mut masked = String::new();
        let mut focus = 0usize;
        let mut at = line_start;
        for other in rows
            .iter()
            .filter(|o| o.end > line_start && o.start < line_end)
        {
            let start = other.start.max(at);
            masked.push_str(&text[at..start]);
            if std::ptr::eq(other, row) {
                focus = masked.chars().count();
            }
            masked.push_str(&format!("[{}]", other.label));
            at = other.end.max(at);
        }
        masked.push_str(&text[at.min(line_end)..line_end]);
        let masked = masked.replace('\r', "");
        lines.push(format!(
            "  Ln {:<4} {:<label_col$}  {}",
            row.line,
            row.label,
            window(&masked, focus, WIDTH)
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use redactor_core::Tier;

    fn row(start: usize, end: usize, line: usize, label: &str) -> Row {
        Row {
            start,
            end,
            line,
            kind: "k".into(),
            label: label.into(),
            tier: Tier::Certain,
            detector: "d".into(),
        }
    }

    #[test]
    fn short_lines_are_shown_whole() {
        let text = "a=SECRET b";
        let out = preview(text, &[row(2, 8, 1, "Password")]);
        assert!(out.ends_with("a=[Password] b"), "{out}");
    }

    #[test]
    fn long_lines_are_windowed_around_the_finding() {
        let text = format!("{}SECRET{}", "x".repeat(200), "y".repeat(200));
        let out = preview(&text, &[row(200, 206, 1, "T")]);
        let shown = out.lines().nth(1).unwrap();
        assert!(
            shown.contains("…x") && shown.contains("[T]") && shown.ends_with('…'),
            "{shown}"
        );
        assert!(shown.chars().count() < 120);
    }

    #[test]
    fn crlf_is_not_shown() {
        let text = "a=SECRET\r\nb";
        let out = preview(text, &[row(2, 8, 1, "T")]);
        assert!(!out.contains('\r'));
    }
}
