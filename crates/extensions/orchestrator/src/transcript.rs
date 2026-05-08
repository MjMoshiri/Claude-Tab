use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Entry {
    #[serde(rename = "user")]
    User { content: serde_json::Value },
    #[serde(rename = "assistant")]
    Assistant { content: serde_json::Value },
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
        #[serde(default)]
        id: String,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(default)]
        tool_use_id: String,
        content: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

pub struct Tail {
    pub entries: Vec<Entry>,
}

impl Tail {
    pub fn read(path: &Path, n: usize) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut entries: Vec<Entry> = content
            .lines()
            .filter_map(|line| serde_json::from_str::<Entry>(line).ok())
            .collect();
        if entries.len() > n {
            entries = entries.split_off(entries.len() - n);
        }
        Ok(Tail { entries })
    }

    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &serde_json::Value)> + '_ {
        self.entries.iter().filter_map(|e| match e {
            Entry::ToolUse { name, input, .. } => Some((name.as_str(), input)),
            _ => None,
        })
    }

    pub fn last_assistant_text(&self) -> Option<String> {
        for e in self.entries.iter().rev() {
            if let Entry::Assistant { content } = e {
                return Some(extract_text(content));
            }
        }
        None
    }

    pub fn compact_repr(&self, max_chars: usize) -> String {
        let mut out = String::new();
        for e in &self.entries {
            let line = match e {
                Entry::User { content } => format!("[user] {}\n", extract_text(content)),
                Entry::Assistant { content } => format!("[asst] {}\n", extract_text(content)),
                Entry::ToolUse { name, input, .. } => {
                    format!("[tool_use {}] {}\n", name, truncate(input.to_string(), 200))
                }
                Entry::ToolResult { content, .. } => {
                    format!("[tool_result] {}\n", truncate(extract_text(content), 200))
                }
                Entry::Other => continue,
            };
            out.push_str(&line);
            if out.len() > max_chars {
                let cutoff = floor_char_boundary(&out, max_chars);
                out.truncate(cutoff);
                out.push_str("\n[...truncated]");
                break;
            }
        }
        out
    }
}

fn extract_text(v: &serde_json::Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .filter_map(|item| {
                item.get("text").and_then(|t| t.as_str()).map(String::from)
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    v.to_string()
}

fn truncate(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let cutoff = floor_char_boundary(&s, max);
    let mut t = s;
    t.truncate(cutoff);
    t.push_str("...");
    t
}

/// Returns the largest valid char boundary <= `idx`. Mirrors the unstable
/// `str::floor_char_boundary` so we don't panic when slicing UTF-8.
fn floor_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_jsonl(lines: &[&str]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{}", l).unwrap();
        }
        f
    }

    #[test]
    fn reads_tail_n_entries() {
        let f = write_jsonl(&[
            r#"{"type":"user","content":"hi"}"#,
            r#"{"type":"assistant","content":"hello"}"#,
            r#"{"type":"tool_use","name":"Bash","input":{"command":"ls"}}"#,
        ]);
        let tail = Tail::read(f.path(), 2).unwrap();
        assert_eq!(tail.entries.len(), 2);
    }

    #[test]
    fn tool_uses_filters_correctly() {
        let f = write_jsonl(&[
            r#"{"type":"user","content":"hi"}"#,
            r#"{"type":"tool_use","name":"Bash","input":{"command":"gh pr create"}}"#,
        ]);
        let tail = Tail::read(f.path(), 10).unwrap();
        let uses: Vec<_> = tail.tool_uses().collect();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].0, "Bash");
    }

    #[test]
    fn skips_malformed_lines() {
        let f = write_jsonl(&[
            "not json",
            r#"{"type":"assistant","content":"ok"}"#,
        ]);
        let tail = Tail::read(f.path(), 10).unwrap();
        assert_eq!(tail.entries.len(), 1);
    }

    #[test]
    fn truncate_does_not_panic_on_multibyte_boundary() {
        // 4-byte emoji (🦀 = U+1F980) — naïve byte-truncate at 1 panics.
        let s = "🦀🦀🦀🦀🦀".to_string(); // 20 bytes, 5 chars
        let out = truncate(s, 7); // mid-emoji byte boundary
        assert!(out.ends_with("..."));
        // We expect either 1 or 2 emoji + "..." (depends on floor boundary).
        assert!(out.contains("🦀"));
    }

    #[test]
    fn last_assistant_text_returns_latest() {
        let f = write_jsonl(&[
            r#"{"type":"assistant","content":"first"}"#,
            r#"{"type":"user","content":"intermediate"}"#,
            r#"{"type":"assistant","content":"latest"}"#,
        ]);
        let tail = Tail::read(f.path(), 10).unwrap();
        assert_eq!(tail.last_assistant_text().as_deref(), Some("latest"));
    }

    #[test]
    fn compact_repr_does_not_panic_on_multibyte_boundary() {
        // Build a transcript whose user message is mostly emoji so the
        // accumulated `out` exceeds max_chars at a byte index that
        // straddles a multi-byte codepoint. The naïve String::truncate
        // would panic; the floor_char_boundary path must succeed.
        let f = write_jsonl(&[r#"{"type":"user","content":"🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀"}"#]);
        let tail = Tail::read(f.path(), 10).unwrap();
        // 10 emoji * 4 bytes + "[user] " prefix far exceeds 10 bytes;
        // cut lands mid-emoji.
        let out = tail.compact_repr(10);
        assert!(out.contains("[...truncated]"), "got: {out:?}");
        // Must not panic and must produce valid UTF-8 (already true by
        // virtue of String, but assert the truncation actually shortened).
        assert!(out.len() < 1000, "got: {out:?}");
    }
}
