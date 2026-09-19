//! TAPI text-based dylib stub (.tbd) files.
//!
//! SDKs don't ship dylib binaries; each dylib is described by a YAML file
//! giving its install name, exported symbols and reexports. We don't need
//! a general YAML parser: TAPI files are machine-generated and regular, so
//! a line-oriented scan is enough.
//!
//! A .tbd file may contain multiple YAML documents: the first one is the
//! library itself, and the rest are the libraries it reexports, inlined.
//! Since reexported symbols resolve through the top-level library, all
//! documents' exports are merged.

use crate::fatal;
use crate::macho::files::FileName;
use crate::mapped_file::MappedFile;

#[derive(Debug, Default, Clone)]
pub struct TbdFile {
    pub install_name: String,
    pub current_version: u32,
    pub exports: Vec<&'static str>,
    pub weak_exports: Vec<&'static str>,
    /// Exports that are thread-local variables (listed separately in
    /// .tbd files; a TLV can only be referenced through TLV
    /// relocations).
    pub tlv_exports: Vec<&'static str>,
    /// The library was built without -application_extension.
    pub not_app_extension_safe: bool,
    /// Install names of reexported libraries described in *other* files
    /// (reexports inlined as documents in this file are already merged
    /// into `exports`).
    pub external_reexports: Vec<&'static str>,
}

/// A JSON value, as much of JSON as a TBD v5 file uses. Strings borrow
/// from the file (input files are leaked); one with an escape is
/// unescaped into a leaked copy.
enum Json {
    Null,
    Bool,
    Num,
    Str(&'static str),
    Arr(Vec<Json>),
    Obj(Vec<(&'static str, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| *k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(items) => items,
            _ => &[],
        }
    }
    fn str(&self) -> Option<&'static str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    /// The strings of an array-valued key.
    fn strs(&self, key: &str) -> impl Iterator<Item = &'static str> + '_ {
        self.get(key).map(Json::arr).unwrap_or(&[]).iter().filter_map(Json::str)
    }
}

struct JsonParser<'a> {
    file: &'a str,
    text: &'static str,
    pos: usize,
}

impl JsonParser<'_> {
    fn fail(&self, what: &str) -> ! {
        fatal!("{}: malformed .tbd JSON at byte {}: {what}", self.file, self.pos);
    }

    fn skip_ws(&mut self) {
        let b = self.text.as_bytes();
        while self.pos < b.len() && matches!(b[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.skip_ws();
        if self.text.as_bytes().get(self.pos) == Some(&c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Json {
        self.skip_ws();
        let b = self.text.as_bytes();
        match b.get(self.pos) {
            Some(b'{') => {
                self.pos += 1;
                let mut fields = Vec::new();
                if !self.eat(b'}') {
                    loop {
                        self.skip_ws();
                        let key = self.string();
                        if !self.eat(b':') {
                            self.fail("expected ':'");
                        }
                        let val = self.value();
                        fields.push((key, val));
                        if self.eat(b',') {
                            continue;
                        }
                        if self.eat(b'}') {
                            break;
                        }
                        self.fail("expected ',' or '}'");
                    }
                }
                Json::Obj(fields)
            }
            Some(b'[') => {
                self.pos += 1;
                let mut items = Vec::new();
                if !self.eat(b']') {
                    loop {
                        items.push(self.value());
                        if self.eat(b',') {
                            continue;
                        }
                        if self.eat(b']') {
                            break;
                        }
                        self.fail("expected ',' or ']'");
                    }
                }
                Json::Arr(items)
            }
            Some(b'"') => Json::Str(self.string()),
            Some(b't') if self.text[self.pos..].starts_with("true") => {
                self.pos += 4;
                Json::Bool
            }
            Some(b'f') if self.text[self.pos..].starts_with("false") => {
                self.pos += 5;
                Json::Bool
            }
            Some(b'n') if self.text[self.pos..].starts_with("null") => {
                self.pos += 4;
                Json::Null
            }
            Some(_) => {
                let start = self.pos;
                while self.pos < b.len()
                    && matches!(b[self.pos], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    self.pos += 1;
                }
                if self.text[start..self.pos].parse::<f64>().is_err() {
                    self.fail("expected a value");
                }
                Json::Num
            }
            None => self.fail("unexpected end of file"),
        }
    }

    fn string(&mut self) -> &'static str {
        let b = self.text.as_bytes();
        if b.get(self.pos) != Some(&b'"') {
            self.fail("expected a string");
        }
        self.pos += 1;
        let start = self.pos;
        let mut escaped = false;
        while self.pos < b.len() && b[self.pos] != b'"' {
            if b[self.pos] == b'\\' {
                escaped = true;
                self.pos += 1;
            }
            self.pos += 1;
        }
        if self.pos >= b.len() {
            self.fail("unterminated string");
        }
        let raw = &self.text[start..self.pos];
        self.pos += 1;
        if !escaped {
            return raw;
        }
        let mut out = String::with_capacity(raw.len());
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        Some(ch) => out.push(ch),
                        None => self.fail("bad \\u escape"),
                    }
                }
                Some(other) => out.push(other),
                None => self.fail("bad escape"),
            }
        }
        String::leak(out)
    }
}

/// Parses a TBD v5 file: JSON with a "main_library" object and, for
/// reexported libraries inlined in the same file, a "libraries" array
/// of objects of the same shape. Each group applies only to its targets.
fn parse_json(file: &str, text: &'static str, arch: &str) -> TbdFile {
    let mut p = JsonParser { file, text, pos: 0 };
    let root = p.value();

    let mut tbd = TbdFile {
        install_name: String::new(),
        current_version: crate::macho::format::encode_version(1, 0, 0),
        exports: Vec::new(),
        weak_exports: Vec::new(),
        tlv_exports: Vec::new(),
        not_app_extension_safe: false,
        external_reexports: Vec::new(),
    };

    let target_of = |lib: &Json| {
        let available = lib
            .get("target_info")
            .map(Json::arr)
            .unwrap_or(&[])
            .iter()
            .filter_map(|t| t.get("target").and_then(Json::str))
            .filter_map(|t| t.strip_suffix("-macos"));
        format!("{}-macos", select_arch(arch, available))
    };
    let applies = |group: &Json, target: &str| {
        group.get("targets").is_none() || group.strs("targets").any(|t| t == target)
    };
    let library_applies = |lib: &Json, target: &str| {
        lib.get("target_info").is_none_or(|info| {
            info.arr().iter().any(|t| t.get("target").and_then(Json::str) == Some(target))
        })
    };

    // Adds one library object's symbols for the requested target.
    let add_symbols = |tbd: &mut TbdFile, lib: &Json| {
        let target = target_of(lib);
        if !library_applies(lib, &target) {
            return;
        }
        for key in ["exported_symbols", "reexported_symbols"] {
            for group in
                lib.get(key).map(Json::arr).unwrap_or(&[]).iter().filter(|g| applies(g, &target))
            {
                for section in ["data", "text"] {
                    let Some(kinds) = group.get(section) else { continue };
                    tbd.exports.extend(kinds.strs("global"));
                    tbd.weak_exports.extend(kinds.strs("weak"));
                    tbd.tlv_exports.extend(kinds.strs("thread_local"));
                    for name in kinds.strs("objc_class") {
                        tbd.exports.push(String::leak(format!("_OBJC_CLASS_$_{name}")));
                        tbd.exports.push(String::leak(format!("_OBJC_METACLASS_$_{name}")));
                    }
                    for name in kinds.strs("objc_eh_type") {
                        tbd.exports.push(String::leak(format!("_OBJC_EHTYPE_$_{name}")));
                    }
                    for name in kinds.strs("objc_ivar") {
                        tbd.exports.push(String::leak(format!("_OBJC_IVAR_$_{name}")));
                    }
                }
            }
        }
    };

    let Some(main) = root.get("main_library") else {
        fatal!("{file}: no main_library in .tbd file");
    };
    let target = target_of(main);
    if !library_applies(main, &target) {
        fatal!("{file}: .tbd file does not support {target}");
    }
    if let Some(name) = main
        .get("install_names")
        .map(Json::arr)
        .and_then(|a| a.iter().find(|g| applies(g, &target)))
        && let Some(s) = name.get("name").and_then(Json::str)
    {
        tbd.install_name = s.to_string();
    }
    if let Some(v) = main
        .get("current_versions")
        .map(Json::arr)
        .and_then(|a| a.iter().find(|g| applies(g, &target)))
        && let Some(s) = v.get("version").and_then(Json::str)
    {
        tbd.current_version = parse_version(s);
    }
    for flags in main.get("flags").map(Json::arr).unwrap_or(&[]) {
        if applies(flags, &target)
            && flags.strs("attributes").any(|a| a == "not_app_extension_safe")
        {
            tbd.not_app_extension_safe = true;
        }
    }
    add_symbols(&mut tbd, main);

    // Reexported libraries: those inlined in "libraries" merge in here;
    // the others live in files of their own.
    let mut doc_names: Vec<&'static str> = Vec::new();
    for lib in root.get("libraries").map(Json::arr).unwrap_or(&[]) {
        for name in lib.get("install_names").map(Json::arr).unwrap_or(&[]) {
            if let Some(s) = name.get("name").and_then(Json::str) {
                doc_names.push(s);
            }
        }
        add_symbols(&mut tbd, lib);
    }
    for group in main
        .get("reexported_libraries")
        .map(Json::arr)
        .unwrap_or(&[])
        .iter()
        .filter(|g| applies(g, &target))
    {
        for name in group.strs("names") {
            if !doc_names.contains(&name) && !tbd.external_reexports.contains(&name) {
                tbd.external_reexports.push(name);
            }
        }
    }

    if tbd.install_name.is_empty() {
        fatal!("{file}: no install name in .tbd file");
    }
    tbd
}

/// Strips a YAML scalar's surrounding quotes, if any.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .or_else(|| s.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
        .unwrap_or(s)
}

fn memchr_from(bytes: &[u8], needle: u8, from: usize) -> Option<usize> {
    if from >= bytes.len() {
        return None;
    }
    // SAFETY: memchr reads within the given range.
    let p = unsafe {
        libc::memchr(bytes.as_ptr().add(from) as *const _, needle as i32, bytes.len() - from)
    };
    if p.is_null() { None } else { Some(p as usize - bytes.as_ptr() as usize) }
}

pub fn parse_version(val: &str) -> u32 {
    let mut nums = val.split('.').map(|s| s.parse().unwrap_or(0));
    let major = nums.next().unwrap_or(1);
    let minor = nums.next().unwrap_or(0);
    let patch = nums.next().unwrap_or(0);
    crate::macho::format::encode_version(major, minor, patch)
}

/// Parses a .tbd file, merging exports of all its documents.
/// A memoized parse. Stub parsing is pure string work over the mapped
/// file, so results are cached by the file's address and target architecture.
/// The linker currently supports only the macOS platform. The big SDK
/// stubs (libSystem's tree, framework umbrellas) can be parsed once,
/// in parallel, by prefetch() before the serial input loop needs them.
pub fn parse_cached(mf: &'static MappedFile, arch: &'static str) -> TbdFile {
    static CACHE: std::sync::Mutex<Option<hashbrown::HashMap<(usize, &'static str), TbdFile>>> =
        std::sync::Mutex::new(None);
    let key = (mf.data().as_ptr() as usize, arch);
    if let Some(tbd) = CACHE.lock().unwrap().get_or_insert_with(hashbrown::HashMap::new).get(&key) {
        return tbd.clone();
    }
    let tbd = parse(mf, arch);
    CACHE.lock().unwrap().get_or_insert_with(hashbrown::HashMap::new).insert(key, tbd.clone());
    tbd
}

/// Warms the parse cache on all cores.
pub fn prefetch(mfs: &[&'static MappedFile], arch: &'static str) -> Vec<TbdFile> {
    use rayon::prelude::*;
    mfs.par_iter().map(|mf| parse_cached(mf, arch)).collect()
}

pub fn parse(mf: &'static MappedFile, arch: &str) -> TbdFile {
    let Ok(text): Result<&'static str, _> = std::str::from_utf8(mf.data()) else {
        fatal!("{}: invalid UTF-8 in .tbd file", mf.name_str());
    };

    // TBD version 5 is JSON (tapi's current output, and what Xcode
    // writes for the "eager linking" stubs of frameworks built in the
    // same workspace); versions 1-4 are YAML.
    if text.trim_start().starts_with('{') {
        return parse_json(mf.name_str(), text, arch);
    }

    let mut tbd = TbdFile {
        install_name: String::new(),
        current_version: crate::macho::format::encode_version(1, 0, 0),
        exports: Vec::new(),
        weak_exports: Vec::new(),
        tlv_exports: Vec::new(),
        not_app_extension_safe: false,
        external_reexports: Vec::new(),
    };

    let mut doc_names: Vec<&'static str> = Vec::new();
    let mut reexports: Vec<&'static str> = Vec::new();

    for (doc, fields) in yaml_documents(text).iter().enumerate() {
        let available = fields.iter().filter(|f| f.indent == 0 && !f.item).flat_map(|f| {
            f.items().filter_map(move |s| match f.key {
                "targets" => s.strip_suffix("-macos"),
                "archs" => Some(s),
                _ => None,
            })
        });
        let arch = select_arch(arch, available);
        let doc_active = yaml_matches(fields.iter().filter(|f| f.indent == 0 && !f.item), arch);
        if doc == 0 && !doc_active {
            fatal!("{}: .tbd file does not support {arch}-macos", mf.name_str());
        }
        let mut active = doc_active;
        for (i, field) in fields.iter().enumerate() {
            if field.indent == 0 && !field.item {
                active = doc_active;
            }
            if field.item {
                let end = fields[i + 1..]
                    .iter()
                    .position(|f| f.indent <= field.indent)
                    .map_or(fields.len(), |n| i + 1 + n);
                active = doc_active && yaml_matches(fields[i..end].iter(), arch);
            }
            if field.key == "install-name" {
                doc_names.push(unquote(field.value));
                if doc == 0 {
                    tbd.install_name = unquote(field.value).to_string();
                }
            }
            if !active {
                continue;
            }
            match field.key {
                "current-version" if doc == 0 => {
                    tbd.current_version = parse_version(unquote(field.value))
                }
                "flags" if doc == 0 => {
                    tbd.not_app_extension_safe =
                        field.items().any(|s| s == "not_app_extension_safe");
                }
                "symbols" => tbd.exports.extend(field.items()),
                "weak-symbols" | "weak-def-symbols" => tbd.weak_exports.extend(field.items()),
                "thread-local-symbols" => tbd.tlv_exports.extend(field.items()),
                "libraries" | "re-exports" if doc == 0 => reexports.extend(field.items()),
                "objc-classes" => {
                    for item in field.items() {
                        tbd.exports.push(String::leak(format!("_OBJC_CLASS_$_{item}")));
                        tbd.exports.push(String::leak(format!("_OBJC_METACLASS_$_{item}")));
                    }
                }
                "objc-eh-types" => {
                    for item in field.items() {
                        tbd.exports.push(String::leak(format!("_OBJC_EHTYPE_$_{item}")));
                    }
                }
                "objc-ivars" => {
                    for item in field.items() {
                        tbd.exports.push(String::leak(format!("_OBJC_IVAR_$_{item}")));
                    }
                }
                _ => {}
            }
        }
    }

    // Reexported libraries not inlined as documents live in files of
    // their own and must be loaded separately.
    tbd.external_reexports =
        reexports.into_iter().filter(|name| !doc_names.contains(name)).collect();

    if tbd.install_name.is_empty() {
        fatal!("{}: no install-name in .tbd file", mf.name_str());
    }
    tbd
}

// Retain indentation and list-item boundaries so a target selector applies
// to its whole group, even when it follows the symbols it qualifies.
struct YamlField {
    indent: usize,
    item: bool,
    key: &'static str,
    value: &'static str,
}

impl YamlField {
    fn items(&self) -> impl Iterator<Item = &'static str> {
        self.value
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(unquote)
            .filter(|s| !s.is_empty())
    }
}

fn yaml_matches<'a>(fields: impl Iterator<Item = &'a YamlField>, arch: &str) -> bool {
    let target = format!("{arch}-macos");
    fields.into_iter().all(|field| match field.key {
        "targets" => field.items().any(|s| s == target),
        "archs" => field.items().any(|s| s == arch),
        "platform" => matches!(unquote(field.value), "macosx" | "macos"),
        _ => true,
    })
}

fn yaml_documents(text: &'static str) -> Vec<Vec<YamlField>> {
    let bytes = text.as_bytes();
    let mut docs = vec![Vec::new()];
    let mut pos = 0;
    while pos < bytes.len() {
        let eol = memchr_from(bytes, b'\n', pos).unwrap_or(bytes.len());
        let raw = &text[pos..eol];
        let line = raw.trim_start();
        let mut next = eol + 1;
        if line.starts_with("---") {
            if !docs.last().unwrap().is_empty() {
                docs.push(Vec::new());
            }
        } else if let Some((key, value)) = line.strip_prefix("- ").unwrap_or(line).split_once(':') {
            let mut value = value.trim();
            // Flow lists may span lines. Consume them once as a single field.
            let value_offset = value.as_ptr() as usize - text.as_ptr() as usize;
            let rest = text[value_offset..].trim_start();
            if rest.starts_with('[')
                && let Some(close) = rest.find(']')
            {
                value = &rest[..close + 1];
                let end = value.as_ptr() as usize - text.as_ptr() as usize + value.len();
                next = memchr_from(bytes, b'\n', end).map_or(bytes.len(), |i| i + 1);
            }
            docs.last_mut().unwrap().push(YamlField {
                indent: raw.len() - line.len(),
                item: line.starts_with("- "),
                key,
                value,
            });
        }
        pos = next;
    }
    docs
}

// Apple's macOS SDK describes many system libraries only as arm64e.
// Use that ABI-compatible slice for arm64 only when no arm64 slice exists.
fn select_arch<'a>(arch: &'a str, available: impl Iterator<Item = &'a str>) -> &'a str {
    let mut exact = false;
    let mut arm64e = false;
    for name in available {
        exact |= name == arch;
        arm64e |= name == "arm64e";
    }
    if arch == "arm64" && !exact && arm64e { "arm64e" } else { arch }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapped(text: &'static str) -> &'static MappedFile {
        crate::mapped_file::MappedFile::from_static(
            ("test.tbd".to_string()).into(),
            text.as_bytes().to_vec().leak(),
        )
    }

    #[test]
    fn yaml_target_groups_and_cache() {
        let mf = mapped(
            r#"--- !tapi-tbd
targets: [ arm64-macos, x86_64-macos ]
install-name: /libtest
exports:
  - targets: [ arm64-macos ]
    symbols: [ _arm ]
    weak-symbols: [ _weak_arm ]
    thread-local-symbols: [ _tls_arm ]
    objc-classes: [ Arm ]
  - symbols: [ _x86 ]
    targets: [
      x86_64-macos
    ]
reexported-libraries:
  - targets: [ arm64-macos ]
    libraries: [ /arm ]
  - targets: [ arm64-ios ]
    libraries: [ /ios ]
"#,
        );
        let arm = parse_cached(mf, "arm64");
        assert_eq!(arm.exports, ["_arm", "_OBJC_CLASS_$_Arm", "_OBJC_METACLASS_$_Arm"]);
        assert_eq!(arm.weak_exports, ["_weak_arm"]);
        assert_eq!(arm.tlv_exports, ["_tls_arm"]);
        assert_eq!(arm.external_reexports, ["/arm"]);
        let x86 = parse_cached(mf, "x86_64");
        assert_eq!(x86.exports, ["_x86"]);
        assert!(x86.weak_exports.is_empty());
        assert!(x86.tlv_exports.is_empty());
        assert!(x86.external_reexports.is_empty());
    }

    #[test]
    fn arm64e_fallback_prefers_exact_architecture() {
        let mf = mapped(
            r#"--- !tapi-tbd
targets: [ arm64-macos, arm64e-macos ]
install-name: /libtest
exports:
  - targets: [ arm64-macos ]
    symbols: [ _arm ]
  - targets: [ arm64e-macos ]
    symbols: [ _arme ]
--- !tapi-tbd
targets: [ arm64e-macos ]
install-name: /inline
exports:
  - targets: [ arm64e-macos ]
    symbols: [ _fallback ]
"#,
        );
        assert_eq!(parse(mf, "arm64").exports, ["_arm", "_fallback"]);
    }

    #[test]
    fn legacy_architecture_groups() {
        let mf = mapped(
            r#"--- !tapi-tbd-v3
archs: [ arm64, x86_64 ]
platform: macosx
install-name: /libtest
exports:
  - archs: [ arm64 ]
    symbols: [ _arm ]
  - archs: [ x86_64 ]
    symbols: [ _x86 ]
"#,
        );
        assert_eq!(parse(mf, "arm64").exports, ["_arm"]);
        assert_eq!(parse(mf, "x86_64").exports, ["_x86"]);
    }

    #[test]
    fn json_target_groups_and_inline_libraries() {
        let mf = mapped(
            r#"{"main_library":{
          "target_info":[{"target":"arm64-macos"},{"target":"x86_64-macos"}],
          "install_names":[{"name":"/libtest"}],
          "exported_symbols":[
            {"data":{"global":["_both"]}},
            {"targets":["arm64-macos"],"data":{"weak":["_weak"],"thread_local":["_tls"]}},
            {"targets":["arm64-ios"],"text":{"global":["_ios"]}}],
          "reexported_libraries":[
            {"targets":["arm64-macos"],"names":["/arm"]},
            {"targets":["x86_64-macos"],"names":["/x86"]}]},
          "libraries":[{"target_info":[{"target":"arm64-macos"}],
            "install_names":[{"name":"/inline"}],
            "exported_symbols":[{"text":{"global":["_inline"]}}]}]}"#,
        );
        let arm = parse(mf, "arm64");
        assert_eq!(arm.exports, ["_both", "_inline"]);
        assert_eq!(arm.weak_exports, ["_weak"]);
        assert_eq!(arm.tlv_exports, ["_tls"]);
        assert_eq!(arm.external_reexports, ["/arm"]);
        let x86 = parse(mf, "x86_64");
        assert_eq!(x86.exports, ["_both"]);
        assert!(x86.weak_exports.is_empty());
        assert!(x86.tlv_exports.is_empty());
        assert_eq!(x86.external_reexports, ["/x86"]);
    }
}
