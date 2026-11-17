use serde_json::{Map, Value};

use crate::error::{Category, Problem, pointer_problem};
use crate::pointer::Pointer;

pub(crate) fn merge(
    target: &mut Map<String, Value>,
    patch: Map<String, Value>,
    at: &Pointer,
    problems: &mut Vec<Problem>,
) {
    for (key, value) in patch {
        let here = at.child(&key);
        match value {
            Value::Null => {
                if target.remove(&key).is_none() {
                    problems.push(without_target(&here));
                }
            }
            Value::Object(patch) => {
                if let Some(Value::Object(existing)) = target.get_mut(&key) {
                    merge(existing, patch, &here, problems);
                } else {
                    let mut fresh = Map::new();
                    merge(&mut fresh, patch, &here, problems);
                    target.insert(key, Value::Object(fresh));
                }
            }
            leaf => {
                reject_nulls(&leaf, &here, problems);
                target.insert(key, leaf);
            }
        }
    }
}

fn reject_nulls(value: &Value, at: &Pointer, problems: &mut Vec<Problem>) {
    match value {
        Value::Null => problems.push(without_target(at)),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                reject_nulls(item, &at.child(index), problems);
            }
        }
        Value::Object(entries) => {
            for (key, item) in entries {
                reject_nulls(item, &at.child(key), problems);
            }
        }
        Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn without_target(at: &Pointer) -> Problem {
    pointer_problem(
        Category::NullWithoutTarget,
        at,
        "`null` removes nothing: no lower layer has this key",
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::merge;
    use crate::pointer::Pointer;

    fn merged(lower: Value, higher: Value) -> (Value, Vec<String>) {
        let mut target = Map::new();
        let mut problems = Vec::new();
        for layer in [lower, higher] {
            let Value::Object(layer) = layer else {
                panic!("layers are maps")
            };
            merge(&mut target, layer, &Pointer::root(), &mut problems);
        }
        let pointers = problems
            .into_iter()
            .map(|problem| problem.location.to_string())
            .collect();
        (Value::Object(target), pointers)
    }

    #[test]
    fn merges_maps_replaces_lists_and_removes_with_null() {
        let (result, problems) = merged(
            json!({"a": {"b": 1, "c": [1, 2], "d": "x"}, "e": "y"}),
            json!({"a": {"b": 2, "c": [3], "d": null}, "f": {"g": true}}),
        );
        assert!(problems.is_empty());
        assert_eq!(
            result,
            json!({"a": {"b": 2, "c": [3]}, "e": "y", "f": {"g": true}})
        );
    }

    #[test]
    fn higher_layer_replaces_on_type_conflict() {
        let (result, problems) = merged(
            json!({"a": "text", "b": {"c": 1}}),
            json!({"a": {"x": 1}, "b": [1]}),
        );
        assert!(problems.is_empty());
        assert_eq!(result, json!({"a": {"x": 1}, "b": [1]}));
    }

    #[test]
    fn reports_null_without_target() {
        let (_, problems) = merged(
            json!({"a": {"b": 1}}),
            json!({"a": {"c": null}, "d": {"e": null}, "f": [1, null], "g": null}),
        );
        assert_eq!(problems, ["#/a/c", "#/d/e", "#/f/1", "#/g"]);
    }
}
