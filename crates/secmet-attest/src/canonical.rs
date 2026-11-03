//! JSON canonique : clés d'objet triées (ordre des octets UTF-8), aucun
//! espace, chaînes échappées par `serde_json`, nombres tels que `serde_json`
//! les émet. Deux documents égaux donnent les mêmes octets, donc la même
//! signature, quelle que soit la bibliothèque qui les a produits.

use serde_json::Value;

/// Sérialise `value` en JSON canonique.
pub fn to_string(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap_or_default());
                out.push(':');
                if let Some(v) = map.get(key) {
                    write(v, out);
                }
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_sorted_and_output_is_compact() {
        let v = json!({"z": 1, "a": {"y": [1, 2, {"b": null, "a": true}], "x": "é\"\n"}, "m": 1.5});
        assert_eq!(
            to_string(&v),
            r#"{"a":{"x":"é\"\n","y":[1,2,{"a":true,"b":null}]},"m":1.5,"z":1}"#
        );
        let other: Value = serde_json::from_str(
            r#"{"m":1.5,"a":{"x":"é\"\n","y":[1,2,{"a":true,"b":null}]},"z":1}"#,
        )
        .unwrap();
        assert_eq!(to_string(&v), to_string(&other));
    }
}
