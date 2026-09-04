//! `.apiworkspace` — workspace/team config (spec section 4's folder
//! structure: "approver keys, relay URL"). Backs spec section 6's
//! conflict-review approver list and the self-hosted-relay connection.
//! Neither of those is wired up yet -- this only gets the file format
//! itself parsing/round-tripping, so a workspace's approver list is
//! something real for those later features to build on, and existing
//! collection discovery already skips it as a dotfile
//! (`crate::collection::discover`).

use indexmap::IndexMap;

use super::blocks::{parse_blocks, parse_key_value_lines, render_key_value_block};
use crate::error::EngineError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMeta {
    pub name: String,
    pub format_version: u32,
}

/// One entry in the approver list -- `public_key` is the base64 form of
/// an `identity::LocalIdentity`'s own `public_key` field, copy-pasted in
/// by whoever grants that person approver status. `name` is a label
/// only; approval checks (once conflict review exists) go by key, never
/// by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approver {
    pub name: String,
    pub public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub meta: WorkspaceMeta,
    /// Spec section 6's room-code-derived relay address (e.g.
    /// `wss://acme-corp.example.com/room/7X9K2Q`). Empty for a
    /// LAN-only workspace with no self-hosted relay configured.
    pub relay_url: String,
    pub approvers: Vec<Approver>,
}

impl WorkspaceFile {
    pub fn parse(input: &str) -> Result<Self, EngineError> {
        let blocks = parse_blocks(input)?;

        let mut meta = None;
        let mut relay_url = String::new();
        let mut approvers = Vec::new();

        for block in blocks {
            match block.name.as_str() {
                "meta" => {
                    let kv = parse_key_value_lines(&block.content);
                    let format_version = kv.get("format_version").and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                    meta = Some(WorkspaceMeta {
                        name: kv.get("name").cloned().unwrap_or_default(),
                        format_version,
                    });
                }
                "relay" => {
                    relay_url = parse_key_value_lines(&block.content).get("url").cloned().unwrap_or_default();
                }
                "approver" => {
                    let kv = parse_key_value_lines(&block.content);
                    let public_key = kv.get("public_key").cloned().unwrap_or_default();
                    // A name-only or key-only block is malformed input,
                    // not a partial approver -- silently dropping it is
                    // safer than half-registering someone's approval
                    // rights on a typo.
                    if !public_key.is_empty() {
                        approvers.push(Approver {
                            name: kv.get("name").cloned().unwrap_or_default(),
                            public_key,
                        });
                    }
                }
                _ => {} // forward-compatible: unknown blocks are ignored rather than erroring
            }
        }

        let meta = meta.unwrap_or(WorkspaceMeta { name: String::new(), format_version: 1 });
        Ok(WorkspaceFile { meta, relay_url, approvers })
    }

    pub fn to_string_pretty(&self) -> String {
        let mut sections = Vec::new();

        let mut meta_kv = IndexMap::new();
        meta_kv.insert("name".to_string(), self.meta.name.clone());
        meta_kv.insert("format_version".to_string(), self.meta.format_version.to_string());
        sections.push(render_key_value_block("meta", &meta_kv).unwrap());

        if !self.relay_url.is_empty() {
            let mut relay_kv = IndexMap::new();
            relay_kv.insert("url".to_string(), self.relay_url.clone());
            sections.push(render_key_value_block("relay", &relay_kv).unwrap());
        }

        for approver in &self.approvers {
            let mut kv = IndexMap::new();
            kv.insert("name".to_string(), approver.name.clone());
            kv.insert("public_key".to_string(), approver.public_key.clone());
            sections.push(render_key_value_block("approver", &kv).unwrap());
        }

        sections.join("\n\n") + "\n"
    }

    /// Adds `public_key` as an approver if it isn't already listed --
    /// "editable by existing approvers" (spec section 6) is a
    /// permission concern the future conflict-review UI enforces; this
    /// just avoids a silent duplicate entry when the same key is added
    /// twice.
    pub fn add_approver(&mut self, name: &str, public_key: &str) {
        if self.approvers.iter().any(|a| a.public_key == public_key) {
            return;
        }
        self.approvers.push(Approver { name: name.to_string(), public_key: public_key.to_string() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"meta {
  name: Acme Corp Workspace
  format_version: 1
}

relay {
  url: wss://acme-corp.example.com/room/7X9K2Q
}

approver {
  name: Alice
  public_key: 3sM9k2QhV8pXz7wYfN4jL6bR1cT5eH0aG9dK8mP2sU4
}

approver {
  name: Bob
  public_key: 8kR3nT7wY2xQ5vL9jH4bM6cF1dS0aG8pU3eK7mN2sT9
}
"#;

    #[test]
    fn parses_example() {
        let ws = WorkspaceFile::parse(EXAMPLE).unwrap();
        assert_eq!(ws.meta.name, "Acme Corp Workspace");
        assert_eq!(ws.meta.format_version, 1);
        assert_eq!(ws.relay_url, "wss://acme-corp.example.com/room/7X9K2Q");
        assert_eq!(ws.approvers.len(), 2);
        assert_eq!(ws.approvers[0].name, "Alice");
        assert_eq!(ws.approvers[1].name, "Bob");
    }

    #[test]
    fn round_trips() {
        let ws = WorkspaceFile::parse(EXAMPLE).unwrap();
        let rendered = ws.to_string_pretty();
        assert_eq!(WorkspaceFile::parse(&rendered).unwrap(), ws);
    }

    #[test]
    fn missing_meta_and_relay_default_rather_than_error() {
        let ws = WorkspaceFile::parse("approver {\n  name: Alice\n  public_key: abc\n}\n").unwrap();
        assert_eq!(ws.meta.name, "");
        assert_eq!(ws.meta.format_version, 1);
        assert_eq!(ws.relay_url, "");
        assert_eq!(ws.approvers.len(), 1);
    }

    #[test]
    fn approver_block_missing_public_key_is_dropped() {
        let ws = WorkspaceFile::parse("approver {\n  name: Alice\n}\n").unwrap();
        assert!(ws.approvers.is_empty());
    }

    #[test]
    fn add_approver_is_idempotent_by_key() {
        let mut ws = WorkspaceFile::parse(EXAMPLE).unwrap();
        let before = ws.approvers.len();
        ws.add_approver("Alice (again)", "3sM9k2QhV8pXz7wYfN4jL6bR1cT5eH0aG9dK8mP2sU4");
        assert_eq!(ws.approvers.len(), before);

        ws.add_approver("Carol", "newkey123");
        assert_eq!(ws.approvers.len(), before + 1);
    }

    #[test]
    fn relay_url_omitted_when_empty() {
        let ws = WorkspaceFile { meta: WorkspaceMeta { name: "x".to_string(), format_version: 1 }, relay_url: String::new(), approvers: vec![] };
        assert!(!ws.to_string_pretty().contains("relay {"));
    }
}
