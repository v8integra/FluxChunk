//! `.apienv` — an environment's committed, shared variables (spec section 4
//! "Secrets: split from structure"). Values may reference a sibling
//! `.apienv.vault` secret via `{{vault:key}}`; that reference is stored
//! here as an ordinary string; only `vault.rs` + `crate::vars` know how to
//! resolve it, and only at send time.

use super::blocks::{parse_blocks, parse_key_value_lines, render_key_value_block};
use crate::error::EngineError;
use indexmap::IndexMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvironmentFile {
    pub vars: IndexMap<String, String>,
}

impl EnvironmentFile {
    pub fn parse(input: &str) -> Result<Self, EngineError> {
        let blocks = parse_blocks(input)?;
        let vars_block = blocks
            .iter()
            .find(|b| b.name == "vars")
            .ok_or_else(|| EngineError::ParseFormat("missing 'vars' block".into()))?;
        Ok(EnvironmentFile {
            vars: parse_key_value_lines(&vars_block.content),
        })
    }

    pub fn to_string_pretty(&self) -> String {
        match render_key_value_block("vars", &self.vars) {
            Some(block) => block + "\n",
            None => "vars {\n}\n".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = "vars {\n  base_url: https://api.local.dev\n  api_key: {{vault:api_key}}\n}\n";

    #[test]
    fn parses_vars_block() {
        let env = EnvironmentFile::parse(EXAMPLE).unwrap();
        assert_eq!(env.vars.get("base_url").unwrap(), "https://api.local.dev");
        assert_eq!(env.vars.get("api_key").unwrap(), "{{vault:api_key}}");
    }

    #[test]
    fn round_trips() {
        let env = EnvironmentFile::parse(EXAMPLE).unwrap();
        let rendered = env.to_string_pretty();
        assert_eq!(EnvironmentFile::parse(&rendered).unwrap(), env);
    }

    #[test]
    fn missing_vars_block_errors() {
        assert!(EnvironmentFile::parse("not_vars {\n  x: 1\n}\n").is_err());
    }
}
