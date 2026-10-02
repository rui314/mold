//! The API list -sdk_imports_api_list reads: a JSON object whose
//! "version" goes into the -sdk_imports report and whose "apis" are the
//! only imports the report lists.
//!
//! ld-prime reads the file with Foundation's NSJSONSerialization, which
//! allows a comma before a closing bracket, and turns the result into
//! dyld's JSON nodes: a node is a map, an array or a value (a string, or
//! a number or boolean as a string), an empty map, array or string is an
//! empty node, and a null is no node at all, which fails the read.

use std::path::Path;

use serde_json::Value;

/// An API list: its version, and the symbol names it lists.
#[derive(Clone, Debug, Default)]
pub struct ApiList {
    pub version: i32,
    pub apis: hashbrown::HashSet<Vec<u8>>,
}

/// Reads an API list, stopping the link on one ld-prime refuses.
pub fn read(path: &Path) -> ApiList {
    let fail = |what: &dyn std::fmt::Display| -> ! {
        crate::fatal!("-sdk_imports_api_list invalid list at {}: {what}", path.display());
    };
    // A file that can't be read is as an empty one.
    let data = std::fs::read(path).unwrap_or_default();
    let root = parse(&data).unwrap_or_else(|e| fail(&e));
    if has_null(&root) {
        fail(&"null value");
    }
    // The version is a value's integer, as atoi reads it.
    let Some(version) = value(&root["version"]).filter(|v| !v.is_empty()) else {
        fail(&"no version");
    };
    // The APIs are the values of an "apis" array.
    let items = root["apis"].as_array().map_or(&[][..], Vec::as_slice);
    let apis: hashbrown::HashSet<Vec<u8>> =
        items.iter().filter_map(value).filter(|name| !name.is_empty()).collect();
    if apis.is_empty() {
        fail(&"no APIs listed");
    }
    ApiList { version: atoi(&version), apis }
}

/// Parses JSON as NSJSONSerialization does: past a byte order mark, and
/// with a comma before a closing bracket or brace allowed.
fn parse(data: &[u8]) -> serde_json::Result<Value> {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let mut text = Vec::with_capacity(data.len());
    let (mut in_string, mut escaped) = (false, false);
    for (i, &c) in data.iter().enumerate() {
        let closes = || matches!(data[i + 1..].trim_ascii_start().first(), Some(b']' | b'}'));
        if in_string {
            in_string = escaped || c != b'"';
            escaped = !escaped && c == b'\\';
        } else if c == b'"' {
            in_string = true;
        } else if c == b',' && closes() {
            continue;
        }
        text.push(c);
    }
    serde_json::from_slice(&text)
}

fn has_null(node: &Value) -> bool {
    match node {
        Value::Null => true,
        Value::Array(items) => items.iter().any(has_null),
        Value::Object(map) => map.values().any(has_null),
        _ => false,
    }
}

/// A value node's string: a string's bytes, a boolean as 1 or 0, and a
/// number as NSNumber's stringValue spells it for atoi (an integer as
/// is, any other by its integral part).
fn value(node: &Value) -> Option<Vec<u8>> {
    let s = match node {
        Value::String(s) => s.clone(),
        Value::Bool(b) => (*b as u8).to_string(),
        Value::Number(n) if n.is_f64() => (n.as_f64().unwrap().trunc() as i64).to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    Some(s.into_bytes())
}

/// C's atoi: blanks, a sign and digits, the rest ignored; the value is
/// clamped to a long, as strtol does, and then cut to an int.
fn atoi(s: &[u8]) -> i32 {
    let s = s.trim_ascii_start();
    let (neg, digits) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut n: i64 = 0;
    for &c in digits.iter().take_while(|c| c.is_ascii_digit()) {
        let d = (c - b'0') as i64;
        n = n.saturating_mul(10);
        n = if neg { n.saturating_sub(d) } else { n.saturating_add(d) };
    }
    n as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version_of(text: &str) -> i32 {
        atoi(&value(&parse(text.as_bytes()).unwrap()["version"]).unwrap())
    }

    #[test]
    fn versions_read_as_atoi_reads_them() {
        assert_eq!(version_of(r#"{"version": 3}"#), 3);
        assert_eq!(version_of(r#"{"version": 3.5}"#), 3);
        assert_eq!(version_of(r#"{"version": 1e3}"#), 1000);
        assert_eq!(version_of(r#"{"version": "3x"}"#), 3);
        assert_eq!(version_of(r#"{"version": "  -7"}"#), -7);
        assert_eq!(version_of(r#"{"version": true}"#), 1);
        assert_eq!(version_of(r#"{"version": 2147483648}"#), -2147483648);
        assert_eq!(version_of(r#"{"version": 4294967296}"#), 0);
        assert_eq!(version_of(r#"{"version": 99999999999999999999}"#), -1);
        assert_eq!(version_of(r#"{"version": -99999999999999999999}"#), 0);
        assert_eq!(version_of("\u{feff}{\"version\": 1, \"version\": 2,}"), 2);
    }

    #[test]
    fn trailing_commas_only_outside_strings() {
        let v = parse(br#"{"a": [1, 2 , ], "b": "x,]\",}" , }"#).unwrap();
        assert_eq!(v["a"].as_array().unwrap().len(), 2);
        assert_eq!(v["b"], "x,]\",}");
        assert!(parse(br#"{"a": 1 "b": 2}"#).is_err());
        assert!(parse(b"{\"a\": 1}\n\n x").is_err());
        assert!(parse(b"").is_err());
    }
}
