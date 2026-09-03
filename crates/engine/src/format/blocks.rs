//! Generic parser for the `.apireq`-family block format:
//!
//! ```text
//! blockname {
//!   raw content, brace-depth aware
//! }
//! ```
//!
//! This layer doesn't know what any block *means* — it just splits the file
//! into named, raw-text blocks. Typed parsing (e.g. `apireq.rs`) builds on
//! top of this. Keeping this split means the CRDT/merge system described in
//! the spec can operate at this same block-boundary granularity later.

use crate::error::EngineError;
use indexmap::IndexMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawBlock {
    pub name: String,
    pub content: String,
}

/// Splits a `.apireq`/`.apicol`/`.apienv` source string into top-level blocks.
pub fn parse_blocks(input: &str) -> Result<Vec<RawBlock>, EngineError> {
    let mut blocks = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let len = chars.len();

    while i < len {
        // Skip whitespace/blank lines between blocks.
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= len {
            break;
        }

        // Read the block name up to '{'.
        let name_start = i;
        while i < len && chars[i] != '{' {
            i += 1;
        }
        if i >= len {
            return Err(EngineError::ParseFormat(format!(
                "expected '{{' after block name near byte offset {name_start}"
            )));
        }
        let name: String = chars[name_start..i].iter().collect::<String>().trim().to_string();
        if name.is_empty() {
            return Err(EngineError::ParseFormat(format!(
                "empty block name near offset {name_start}"
            )));
        }

        // Consume the opening brace, then scan for the matching close.
        i += 1; // skip '{'
        let content_start = i;
        let mut depth = 1;
        while i < len && depth > 0 {
            match chars[i] {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            i += 1;
        }
        if depth != 0 {
            return Err(EngineError::ParseFormat(format!(
                "unterminated block '{name}' (missing closing '}}')"
            )));
        }
        let raw_content: String = chars[content_start..i].iter().collect();
        i += 1; // skip closing '}'

        blocks.push(RawBlock {
            name,
            content: trim_block_content(&raw_content),
        });
    }

    Ok(blocks)
}

/// Blocks are written as `{\n  ...content...\n}` — strip exactly the
/// cosmetic leading/trailing newline so round-tripping doesn't accumulate
/// blank lines, but otherwise leave content (including internal blank
/// lines/indentation) untouched.
fn trim_block_content(raw: &str) -> String {
    let s = raw.strip_prefix('\n').unwrap_or(raw);
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.to_string()
}

/// Parses a block's raw content as `key: value` lines (used by `meta`,
/// `headers`, `params:*`, HTTP-method blocks, `vars`). Preserves order and
/// tolerates values containing `:` (splits on the first colon only).
pub fn parse_key_value_lines(content: &str) -> IndexMap<String, String> {
    let mut map = IndexMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    map
}

/// Renders a block back out in canonical `name {\n  key: value\n}` form.
/// Returns `None` if there's nothing to write (caller should omit the block).
pub fn render_key_value_block(name: &str, map: &IndexMap<String, String>) -> Option<String> {
    if map.is_empty() {
        return None;
    }
    let mut out = format!("{name} {{\n");
    for (k, v) in map {
        out.push_str(&format!("  {k}: {v}\n"));
    }
    out.push('}');
    Some(out)
}

/// Renders a raw-content block (body/script) back out, indenting nothing —
/// the content is kept byte-for-byte as authored.
pub fn render_raw_block(name: &str, content: &str) -> String {
    format!("{name} {{\n{content}\n}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_simple_blocks() {
        let input = "meta {\n  name: Get user\n  seq: 3\n}\n\nget {\n  url: {{base_url}}/users\n}\n";
        let blocks = parse_blocks(input).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].name, "meta");
        assert_eq!(blocks[1].name, "get");
    }

    #[test]
    fn handles_nested_braces_in_json_body() {
        let input = "body:json {\n  {\n    \"include\": [\"profile\", \"roles\"]\n  }\n}\n";
        let blocks = parse_blocks(input).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].name, "body:json");
        assert!(blocks[0].content.contains("\"include\""));
    }

    #[test]
    fn errors_on_unterminated_block() {
        let input = "meta {\n  name: broken\n";
        assert!(parse_blocks(input).is_err());
    }
}
