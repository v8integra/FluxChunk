//! Structural JSON diff for response history comparison (spec section 10:
//! "Structural diff (not text diff) between any two history entries...
//! recursive, walks nested objects/arrays, color-coded like a Git diff").
//!
//! The whole tree is always returned, not just the changed parts — every
//! node (unchanged included) carries a status, so a UI can render the
//! complete structure with diff markers rather than only a change list.
//! Arrays are compared positionally by index; this is a deliberate
//! simplification over an LCS-style "smart" array diff (which would
//! better handle an element merely moving position), matching what the
//! spec asks for without building a much larger algorithm for a case
//! JSON API responses rarely hit in practice.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffStatus {
    Added,
    Removed,
    Changed,
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffNode {
    /// Object key or array index (as a string); `None` only at the root.
    pub key: Option<String>,
    pub status: DiffStatus,
    /// Populated only on leaf nodes (scalars) -- object/array nodes carry
    /// their comparison entirely in `children`, not a wholesale
    /// before/after value.
    pub old_value: Option<Value>,
    pub new_value: Option<Value>,
    pub children: Vec<DiffNode>,
}

pub fn diff_json(old: &Value, new: &Value) -> DiffNode {
    diff_value(None, Some(old), Some(new))
}

fn diff_value(key: Option<String>, old: Option<&Value>, new: Option<&Value>) -> DiffNode {
    match (old, new) {
        (None, None) => unreachable!("diff_value called with neither side present"),
        (None, Some(v)) => leaf_or_subtree(key, v, DiffStatus::Added, false),
        (Some(v), None) => leaf_or_subtree(key, v, DiffStatus::Removed, true),
        (Some(old_val), Some(new_val)) => match (old_val, new_val) {
            (Value::Object(old_map), Value::Object(new_map)) => {
                let mut keys: BTreeSet<&String> = old_map.keys().collect();
                keys.extend(new_map.keys());
                let children: Vec<DiffNode> =
                    keys.into_iter().map(|k| diff_value(Some(k.clone()), old_map.get(k), new_map.get(k))).collect();
                DiffNode {
                    key,
                    status: overall_status(&children),
                    old_value: None,
                    new_value: None,
                    children,
                }
            }
            (Value::Array(old_arr), Value::Array(new_arr)) => {
                let len = old_arr.len().max(new_arr.len());
                let children: Vec<DiffNode> =
                    (0..len).map(|i| diff_value(Some(i.to_string()), old_arr.get(i), new_arr.get(i))).collect();
                DiffNode {
                    key,
                    status: overall_status(&children),
                    old_value: None,
                    new_value: None,
                    children,
                }
            }
            // Different shapes at the same key (e.g. a field that used to
            // be a string and is now an object) -- don't try to recurse
            // into incompatible structures, just report the whole thing
            // as changed.
            _ => {
                if old_val == new_val {
                    DiffNode {
                        key,
                        status: DiffStatus::Unchanged,
                        old_value: Some(old_val.clone()),
                        new_value: Some(new_val.clone()),
                        children: vec![],
                    }
                } else {
                    DiffNode {
                        key,
                        status: DiffStatus::Changed,
                        old_value: Some(old_val.clone()),
                        new_value: Some(new_val.clone()),
                        children: vec![],
                    }
                }
            }
        },
    }
}

/// Builds an Added/Removed node for a value that only exists on one
/// side. Recurses into objects/arrays so the *entire* new/gone subtree
/// comes back tagged, not just its root.
fn leaf_or_subtree(key: Option<String>, value: &Value, status: DiffStatus, is_removed: bool) -> DiffNode {
    let side = |v: &Value| Some(v.clone());
    match value {
        Value::Object(map) => {
            let children = map.iter().map(|(k, v)| leaf_or_subtree(Some(k.clone()), v, status, is_removed)).collect();
            DiffNode { key, status, old_value: None, new_value: None, children }
        }
        Value::Array(arr) => {
            let children =
                arr.iter().enumerate().map(|(i, v)| leaf_or_subtree(Some(i.to_string()), v, status, is_removed)).collect();
            DiffNode { key, status, old_value: None, new_value: None, children }
        }
        scalar => DiffNode {
            key,
            status,
            old_value: if is_removed { side(scalar) } else { None },
            new_value: if is_removed { None } else { side(scalar) },
            children: vec![],
        },
    }
}

fn overall_status(children: &[DiffNode]) -> DiffStatus {
    if children.iter().all(|c| c.status == DiffStatus::Unchanged) {
        DiffStatus::Unchanged
    } else {
        DiffStatus::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identical_objects_are_fully_unchanged() {
        let a = json!({"id": 1, "name": "Ada"});
        let diff = diff_json(&a, &a.clone());
        assert_eq!(diff.status, DiffStatus::Unchanged);
        assert_eq!(diff.children.len(), 2);
        assert!(diff.children.iter().all(|c| c.status == DiffStatus::Unchanged));
    }

    #[test]
    fn changed_scalar_field() {
        let old = json!({"status": "pending"});
        let new = json!({"status": "done"});
        let diff = diff_json(&old, &new);
        assert_eq!(diff.status, DiffStatus::Changed);
        let field = &diff.children[0];
        assert_eq!(field.key.as_deref(), Some("status"));
        assert_eq!(field.status, DiffStatus::Changed);
        assert_eq!(field.old_value, Some(json!("pending")));
        assert_eq!(field.new_value, Some(json!("done")));
    }

    #[test]
    fn added_field_marks_only_that_field() {
        let old = json!({"a": 1});
        let new = json!({"a": 1, "b": 2});
        let diff = diff_json(&old, &new);
        assert_eq!(diff.status, DiffStatus::Changed);
        let a = diff.children.iter().find(|c| c.key.as_deref() == Some("a")).unwrap();
        assert_eq!(a.status, DiffStatus::Unchanged);
        let b = diff.children.iter().find(|c| c.key.as_deref() == Some("b")).unwrap();
        assert_eq!(b.status, DiffStatus::Added);
        assert_eq!(b.new_value, Some(json!(2)));
        assert_eq!(b.old_value, None);
    }

    #[test]
    fn removed_field_marks_only_that_field() {
        let old = json!({"a": 1, "b": 2});
        let new = json!({"a": 1});
        let diff = diff_json(&old, &new);
        let b = diff.children.iter().find(|c| c.key.as_deref() == Some("b")).unwrap();
        assert_eq!(b.status, DiffStatus::Removed);
        assert_eq!(b.old_value, Some(json!(2)));
        assert_eq!(b.new_value, None);
    }

    #[test]
    fn added_nested_object_marks_whole_subtree_added() {
        let old = json!({});
        let new = json!({"profile": {"age": 30, "tags": ["x", "y"]}});
        let diff = diff_json(&old, &new);
        let profile = &diff.children[0];
        assert_eq!(profile.status, DiffStatus::Added);
        assert_eq!(profile.children.len(), 2);
        assert!(profile.children.iter().all(|c| c.status == DiffStatus::Added));
        let tags = profile.children.iter().find(|c| c.key.as_deref() == Some("tags")).unwrap();
        assert_eq!(tags.children.len(), 2);
        assert!(tags.children.iter().all(|c| c.status == DiffStatus::Added));
    }

    #[test]
    fn array_elements_diffed_positionally() {
        let old = json!([1, 2, 3]);
        let new = json!([1, 99, 3, 4]);
        let diff = diff_json(&old, &new);
        assert_eq!(diff.status, DiffStatus::Changed);
        assert_eq!(diff.children.len(), 4);
        assert_eq!(diff.children[0].status, DiffStatus::Unchanged);
        assert_eq!(diff.children[1].status, DiffStatus::Changed);
        assert_eq!(diff.children[2].status, DiffStatus::Unchanged);
        assert_eq!(diff.children[3].status, DiffStatus::Added);
        assert_eq!(diff.children[3].new_value, Some(json!(4)));
    }

    #[test]
    fn type_change_at_same_key_is_changed_not_recursed() {
        let old = json!({"value": "42"});
        let new = json!({"value": {"nested": 42}});
        let diff = diff_json(&old, &new);
        let value = &diff.children[0];
        assert_eq!(value.status, DiffStatus::Changed);
        assert!(value.children.is_empty());
        assert_eq!(value.old_value, Some(json!("42")));
        assert_eq!(value.new_value, Some(json!({"nested": 42})));
    }

    #[test]
    fn top_level_scalars_diff_directly() {
        let diff = diff_json(&json!("old"), &json!("new"));
        assert_eq!(diff.key, None);
        assert_eq!(diff.status, DiffStatus::Changed);
    }
}
