//! The API list -sdk_imports_api_list reads: a JSON object whose
//! "version" goes into the -sdk_imports report and whose "apis" are the
//! only imports the report lists.
//!
//! ld-prime reads the file with Foundation's NSJSONSerialization and
//! turns the result into dyld's JSON nodes, whose diagnostics it
//! reports: a node is a map, an array or a value (a string, or a
//! number or boolean as a string), and an empty map or array is an
//! empty node. This module reads it the same way, with the same words
//! for each fault.

use std::path::Path;

/// An API list: its version, and the symbol names it lists.
#[derive(Clone, Debug, Default)]
pub struct ApiList {
    pub version: i32,
    pub apis: hashbrown::HashSet<Vec<u8>>,
}

/// Reads an API list, stopping the link on one ld-prime refuses.
pub fn read(path: &Path) -> ApiList {
    let fail = |what: &str| -> ! {
        crate::fatal!("-sdk_imports_api_list invalid list at {}: {what}", path.display());
    };
    // dyld's diagnostics end with a newline of their own.
    let dyld_fail = |what: &str| -> ! { fail(&format!("{what}\n")) };
    // A file that can't be read is as an empty one.
    let data = std::fs::read(path).unwrap_or_default();
    let node = match parse(&data) {
        Ok(node) => node,
        Err(why) => dyld_fail(&format!(
            "Could not deserialize json file: '{}' because '{why}'",
            path.display()
        )),
    };
    let node = node.unwrap_or_else(|| dyld_fail("Unknown json deserialized type"));
    let version = int_value(key(&node, "version").unwrap_or_else(|e| dyld_fail(&e)))
        .unwrap_or_else(|e| dyld_fail(&e));
    // The APIs are the values of an "apis" array; any other is none.
    let apis: hashbrown::HashSet<Vec<u8>> = match key(&node, "apis") {
        Ok(Node::Array(items)) => (items.iter())
            .filter_map(|item| match item {
                Node::Value(name) => Some(name.clone()),
                _ => None,
            })
            .collect(),
        _ => hashbrown::HashSet::new(),
    };
    if apis.is_empty() {
        fail(&format!("API symbol list {} can't be empty", path.display()));
    }
    ApiList { version, apis }
}

/// A dyld JSON node.
enum Node {
    Map(Vec<(Vec<u8>, Node)>),
    Array(Vec<Node>),
    Value(Vec<u8>),
}

/// A map's value for `key` (the last of the key's, if repeated).
fn key<'a>(node: &'a Node, key: &str) -> Result<&'a Node, String> {
    match node {
        Node::Array(items) if !items.is_empty() => {
            Err(format!("Cannot get key '{key}' from array node"))
        }
        Node::Value(v) if !v.is_empty() => Err(format!("Cannot get key '{key}' from value node")),
        Node::Map(entries) => match entries.iter().rev().find(|(k, _)| k == key.as_bytes()) {
            Some((_, value)) => Ok(value),
            None => Err(format!("Map node doesn't have element for key '{key}'")),
        },
        _ => Err(format!("Map node doesn't have element for key '{key}'")),
    }
}

/// A value node's integer, as atoi reads it into an int.
fn int_value(node: &Node) -> Result<i32, String> {
    match node {
        Node::Array(items) if !items.is_empty() => {
            Err("Cannot get integer value from array node".into())
        }
        Node::Map(entries) if !entries.is_empty() => {
            Err("Cannot get integer value from value node".into())
        }
        Node::Value(v) if !v.is_empty() => Ok(atoi(v)),
        _ => Err("Cannot get integer value from empty node".into()),
    }
}

/// C's atoi: blanks, a sign and digits, the rest ignored, the value
/// cut to an int.
fn atoi(s: &[u8]) -> i32 {
    let s = s.trim_ascii_start();
    let (neg, digits) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut n: i64 = 0;
    for &c in digits.iter().take_while(|c| c.is_ascii_digit()) {
        n = n.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    (if neg { n.wrapping_neg() } else { n }) as i32
}

/// Parses JSON as NSJSONSerialization does, into dyld's nodes: None if
/// the document has a null, which they can't hold. An error is
/// NSJSONSerialization's, as ld-prime spells it.
fn parse(data: &[u8]) -> Result<Option<Node>, String> {
    if data.is_empty() {
        return Err("Error Domain=NSCocoaErrorDomain Code=3840 \"Unable to parse empty data.\" \
                    UserInfo={NSDebugDescription=Unable to parse empty data.}"
            .into());
    }
    let start = if data.starts_with(b"\xef\xbb\xbf") { 3 } else { 0 };
    let mut p = Parser { data, pos: start };
    p.skip_ws();
    let fail = |p: &Parser, what: &str| Err(p.error_at(p.pos, what));
    match data.get(p.pos) {
        None => return fail(&p, "JSON text did not have any content"),
        Some(b'{' | b'[') => {}
        Some(_) => {
            return fail(
                &p,
                "JSON text did not start with array or object and option to allow fragments not set.",
            );
        }
    }
    let node = p.value()?;
    p.skip_ws();
    if p.pos < data.len() {
        return fail(&p, "Garbage at end");
    }
    Ok(node)
}

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    /// NSJSONSerialization's error for `what` at byte `at`: the line and
    /// the column (counted from 0) and the byte index.
    fn error_at(&self, at: usize, what: &str) -> String {
        let before = &self.data[..at];
        let line = before.iter().filter(|&&c| c == b'\n').count() + 1;
        let column = at - before.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
        let desc = format!("{what} around line {line}, column {column}.");
        format!(
            "Error Domain=NSCocoaErrorDomain Code=3840 \"{desc}\" \
             UserInfo={{NSDebugDescription={desc}, NSJSONSerializationErrorIndex={at}}}"
        )
    }

    fn skip_ws(&mut self) {
        while matches!(self.data.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eof(&self) -> String {
        self.error_at(self.data.len(), "Unexpected end of file")
    }

    /// A value, None for a null (or a container with one).
    fn value(&mut self) -> Result<Option<Node>, String> {
        self.skip_ws();
        let Some(&c) = self.data.get(self.pos) else { return Err(self.eof()) };
        match c {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(Some(Node::Value(self.string()?))),
            b't' => self
                .literal(b"true", "Something looked like a 'true' but wasn't")
                .map(|_| Some(Node::Value(b"1".to_vec()))),
            b'f' => self
                .literal(b"false", "Something looked like a 'false' but wasn't")
                .map(|_| Some(Node::Value(b"0".to_vec()))),
            b'n' => {
                self.literal(b"null", "Something looked like a 'null' but wasn't").map(|_| None)
            }
            b'-' | b'0'..=b'9' => self.number().map(|n| Some(Node::Value(n))),
            _ => Err(self.error_at(self.pos, "Invalid value")),
        }
    }

    fn literal(&mut self, word: &[u8], what: &str) -> Result<(), String> {
        if !self.data[self.pos..].starts_with(word) {
            return Err(self.error_at(self.pos, what));
        }
        self.pos += word.len();
        Ok(())
    }

    fn object(&mut self) -> Result<Option<Node>, String> {
        self.pos += 1;
        let mut entries = Vec::new();
        let mut null = false;
        loop {
            self.skip_ws();
            match self.data.get(self.pos) {
                None => return Err(self.eof()),
                Some(b'}') => break,
                Some(b'"') => {}
                Some(_) => return Err(self.error_at(self.pos, "No string key for value in object")),
            }
            let key = self.string()?;
            self.skip_ws();
            match self.data.get(self.pos) {
                None => return Err(self.eof()),
                Some(b':') => self.pos += 1,
                Some(_) => return Err(self.error_at(self.pos, "Badly formed object")),
            }
            match self.value()? {
                Some(value) => entries.push((key, value)),
                None => null = true,
            }
            self.skip_ws();
            match self.data.get(self.pos) {
                None => return Err(self.eof()),
                Some(b',') => self.pos += 1,
                Some(b'}') => break,
                Some(_) => return Err(self.error_at(self.pos, "Badly formed object")),
            }
        }
        self.pos += 1;
        Ok((!null).then_some(Node::Map(entries)))
    }

    fn array(&mut self) -> Result<Option<Node>, String> {
        self.pos += 1;
        let mut items = Vec::new();
        let mut null = false;
        loop {
            self.skip_ws();
            match self.data.get(self.pos) {
                None => return Err(self.eof()),
                Some(b']') => break,
                Some(_) => {}
            }
            match self.value()? {
                Some(item) => items.push(item),
                None => null = true,
            }
            self.skip_ws();
            match self.data.get(self.pos) {
                None => return Err(self.eof()),
                Some(b',') => self.pos += 1,
                Some(b']') => break,
                Some(_) => return Err(self.error_at(self.pos, "Badly formed array")),
            }
        }
        self.pos += 1;
        Ok((!null).then_some(Node::Array(items)))
    }

    fn string(&mut self) -> Result<Vec<u8>, String> {
        let start = self.pos;
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.data.get(self.pos) else {
                return Err(self.error_at(start, "Unterminated string"));
            };
            match c {
                b'"' => break,
                0..0x20 => return Err(self.error_at(self.pos, "Unescaped control character")),
                b'\\' => {
                    let esc = self.pos;
                    let unescaped = match self.data.get(self.pos + 1) {
                        Some(b'"') => b'"',
                        Some(b'\\') => b'\\',
                        Some(b'/') => b'/',
                        Some(b'b') => 8,
                        Some(b'f') => 12,
                        Some(b'n') => b'\n',
                        Some(b'r') => b'\r',
                        Some(b't') => b'\t',
                        Some(b'u') => {
                            let hex = self.data.get(self.pos + 2..self.pos + 6);
                            let code = hex
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .and_then(char::from_u32);
                            let Some(ch) = code else {
                                return Err(self.error_at(esc, "Invalid escape sequence"));
                            };
                            out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
                            self.pos += 6;
                            continue;
                        }
                        _ => return Err(self.error_at(esc, "Invalid escape sequence")),
                    };
                    out.push(unescaped);
                    self.pos += 2;
                    continue;
                }
                _ => out.push(c),
            }
            self.pos += 1;
        }
        self.pos += 1;
        Ok(out)
    }

    /// A number, as NSNumber's stringValue spells it for atoi: an
    /// integer as written, any other by its integral part.
    fn number(&mut self) -> Result<Vec<u8>, String> {
        let start = self.pos;
        let digits = |p: &mut Self| {
            let from = p.pos;
            while matches!(p.data.get(p.pos), Some(b'0'..=b'9')) {
                p.pos += 1;
            }
            p.pos - from
        };
        if self.data[self.pos] == b'-' {
            self.pos += 1;
        }
        if self.data.get(self.pos) == Some(&b'0') {
            self.pos += 1;
            if matches!(self.data.get(self.pos), Some(b'0'..=b'9')) {
                return Err(self.error_at(self.pos, "Number with leading zero"));
            }
        } else if digits(self) == 0 {
            return Err(self.error_at(self.pos, "Number with minus sign but no digits"));
        }
        let mut integral = true;
        if self.data.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            integral = false;
            if digits(self) == 0 {
                return Err(
                    self.error_at(self.pos, "Number with decimal point but no additional digits")
                );
            }
        }
        if matches!(self.data.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            integral = false;
            if matches!(self.data.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            digits(self);
        }
        let text = &self.data[start..self.pos];
        if integral {
            return Ok(text.to_vec());
        }
        let value: f64 = std::str::from_utf8(text).ok().and_then(|t| t.parse().ok()).unwrap_or(0.0);
        if !value.is_finite() {
            return Err(self.error_at(start, "Number wound up as NaN"));
        }
        Ok(format!("{}", value.trunc() as i64).into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version_of(text: &str) -> i32 {
        let node = parse(text.as_bytes()).unwrap().unwrap();
        int_value(key(&node, "version").unwrap()).unwrap()
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
        assert_eq!(version_of(r#"{"version": 1, "version": 2,}"#), 2);
    }

    #[test]
    fn errors_name_the_place() {
        let err = |text: &str| parse(text.as_bytes()).err().unwrap();
        assert!(
            err(r#"{"version": 1 "apis": []}"#)
                .contains("Badly formed object around line 1, column 14.")
        );
        assert!(
            err("{\"a\": 1}\n\n x").contains(
                "Garbage at end around line 3, column 1., NSJSONSerializationErrorIndex=11"
            )
        );
        assert!(err("[1,2").contains("Unexpected end of file around line 1, column 4."));
        assert!(err("   ").contains("JSON text did not have any content around line 1, column 3."));
    }
}
