//! TAPI text-based dylib stub (.tbd) files.
//!
//! SDKs don't ship dylib binaries; each dylib is described by a YAML file
//! giving its install name, exported symbols and reexports. We don't need
//! a general YAML parser: TAPI files are machine-generated and regular, so
//! a line-oriented scan is enough.
//!
//! A .tbd file may contain multiple YAML documents: the first one is the
//! library itself, and the rest are the libraries it reexports, inlined.
//! Each document is kept apart: the linker decides per library whether
//! its symbols resolve through the top-level library or bind to it
//! directly.

use std::path::Path;

use rayon::prelude::*;

use crate::fatal;
use crate::macho::*;
use crate::mapped_file::MappedFile;

#[derive(Debug, Default, Clone)]
pub struct TbdFile {
    pub install_name: String,
    pub current_version: u32,
    /// The compatibility version (1.0.0 when the stub gives none), for
    /// the client's LC_LOAD_DYLIB.
    pub compatibility_version: u32,
    pub exports: Vec<&'static str>,
    pub weak_exports: Vec<&'static str>,
    /// Exports that are thread-local variables (listed separately in
    /// .tbd files; a TLV can only be referenced through TLV
    /// relocations).
    pub tlv_exports: Vec<&'static str>,
    /// The umbrella the library belongs to (parent-umbrella), and the
    /// clients it lets link it directly (allowable-clients).
    pub parent_umbrella: Option<&'static str>,
    pub allowable_clients: Vec<&'static str>,
    /// Install names of the libraries this one re-exports: documents
    /// inlined in the same file and libraries in files of their own
    /// alike. (Every inlined document counts as re-exported, listed
    /// or not.)
    pub reexports: Vec<&'static str>,
    /// The file's other documents - the re-exported libraries tapi
    /// inlined - each parsed on its own. The linker decides per
    /// library whether it loads as a dylib in its own right (a public
    /// location, which ld64 binds to directly) or merges into this one.
    pub documents: Vec<Self>,
    /// The platforms the library has a target for on the architecture,
    /// in platform order. The one read is the link's, or else the
    /// first (which a firmware link takes).
    pub platforms: Vec<u32>,
    /// The minimum OS version of the target read, which only a version 5
    /// file gives (min_deployment); 0 for none.
    pub minos: u32,
}

impl TbdFile {
    /// The inlined document for a re-exported library, by install name.
    pub fn document(&self, install_name: &str) -> Option<&Self> {
        self.documents.iter().find(|d| d.install_name == install_name)
    }
}

/// A JSON value, as much of JSON as a TBD v5 file uses. Strings borrow
/// from the file (input files are leaked); one with an escape is
/// unescaped into a leaked copy.
enum Json {
    Null,
    Bool,
    Num(f64),
    Str(&'static str),
    Arr(Vec<Self>),
    Obj(Vec<(&'static str, Self)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Obj(fields) => fields.iter().find(|(k, _)| *k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn arr(&self) -> &[Self] {
        match self {
            Self::Arr(items) => items,
            _ => &[],
        }
    }
    fn str(&self) -> Option<&'static str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
    /// The strings of an array-valued key.
    fn strs(&self, key: &str) -> impl Iterator<Item = &'static str> {
        self.get(key).map(Self::arr).unwrap_or(&[]).iter().filter_map(Self::str)
    }

    /// The value as an integer, as LLVM's JSON reader takes one: a
    /// number with no fraction.
    fn integer(&self) -> Option<i64> {
        match *self {
            Self::Num(n) if n.fract() == 0.0 && n.abs() < 9.2e18 => Some(n as i64),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    file: &'a Path,
    text: &'static str,
    pos: usize,
}

impl JsonParser<'_> {
    fn fail(&self, what: &str) -> ! {
        fatal!("{}: malformed .tbd JSON at byte {}: {what}", self.file.display(), self.pos);
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
                match self.text[start..self.pos].parse::<f64>() {
                    Ok(n) => Json::Num(n),
                    Err(_) => self.fail("expected a value"),
                }
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

/// Adds the symbols of an Objective-C class a stub lists by name to its
/// exports: the class and metaclass objects, and with `eh_type` the
/// exception type too. tapi has a class listed for its exception type
/// alone (objc-eh-types, objc_eh_type) export all three: ld-prime links
/// _OBJC_CLASS_$_Foo against such a stub.
fn push_objc_class(exports: &mut Vec<&'static str>, name: &str, eh_type: bool) {
    exports.push(String::leak(format!("_OBJC_CLASS_$_{name}")));
    exports.push(String::leak(format!("_OBJC_METACLASS_$_{name}")));
    if eh_type {
        exports.push(String::leak(format!("_OBJC_EHTYPE_$_{name}")));
    }
}

/// Parses a TBD v5 file: JSON with a "main_library" object and, for
/// reexported libraries inlined in the same file, a "libraries" array
/// of objects of the same shape. Each group applies only to its targets.
fn parse_json(
    file: &Path,
    text: &'static str,
    arch: &'static str,
    platform: u32,
) -> Option<TbdFile> {
    let mut p = JsonParser { file, text, pos: 0 };
    let root = p.value();
    check_json(file, &root);

    let targets_of = |lib: &Json| -> Vec<Target> {
        let info = lib.get("target_info").map(Json::arr).unwrap_or(&[]);
        info.iter().filter_map(|t| t.get("target").and_then(Json::str)).filter_map(target).collect()
    };
    let target_of = |lib: &Json| select_target(arch, platform, &targets_of(lib)).0;
    // A group's targets TAPI can't read - not an array, or one with
    // something other than a string - don't restrict it.
    let applies = |group: &Json, want: Target| match group.get("targets") {
        Some(Json::Arr(targets)) if targets.iter().all(|t| t.str().is_some()) => {
            targets.iter().filter_map(Json::str).any(|t| target(t) == Some(want))
        }
        _ => true,
    };
    let library_applies = |lib: &Json, want: Target| {
        lib.get("target_info").is_none() || targets_of(lib).contains(&want)
    };

    // Adds one library object's symbols for the requested target.
    let add_symbols = |tbd: &mut TbdFile, lib: &Json| {
        let target = target_of(lib);
        if !library_applies(lib, target) {
            return;
        }
        for key in ["exported_symbols", "reexported_symbols"] {
            for group in
                lib.get(key).map(Json::arr).unwrap_or(&[]).iter().filter(|g| applies(g, target))
            {
                for section in ["data", "text"] {
                    let Some(kinds) = group.get(section) else { continue };
                    tbd.exports.extend(kinds.strs("global"));
                    tbd.weak_exports.extend(kinds.strs("weak"));
                    tbd.tlv_exports.extend(kinds.strs("thread_local"));
                    for name in kinds.strs("objc_class") {
                        push_objc_class(&mut tbd.exports, name, false);
                    }
                    for name in kinds.strs("objc_eh_type") {
                        push_objc_class(&mut tbd.exports, name, true);
                    }
                    for name in kinds.strs("objc_ivar") {
                        tbd.exports.push(String::leak(format!("_OBJC_IVAR_$_{name}")));
                    }
                }
            }
        }
    };

    // One library object (the main library or an inlined re-export) as
    // a TbdFile: its install name, version, flags, symbols and the
    // names it re-exports, for the requested target.
    let parse_library = |lib: &Json| -> Option<TbdFile> {
        let target = target_of(lib);
        if !library_applies(lib, target) {
            return None;
        }
        let mut tbd = TbdFile {
            current_version: crate::macho::encode_version(1, 0, 0),
            compatibility_version: crate::macho::encode_version(1, 0, 0),
            ..TbdFile::default()
        };
        // TAPI takes the install name and versions from the first entry
        // of their lists, whatever its targets.
        let first = |key: &str, field: &str| {
            lib.get(key).map(Json::arr).and_then(<[Json]>::first)?.get(field)?.str()
        };
        if let Some(s) = first("install_names", "name") {
            tbd.install_name = s.to_string();
        }
        if let Some(s) = first("current_versions", "version") {
            tbd.current_version = packed_version(s).unwrap();
        }
        if let Some(s) = first("compatibility_versions", "version") {
            tbd.compatibility_version = packed_version(s).unwrap();
        }
        let info = lib.get("target_info").map(Json::arr).unwrap_or(&[]);
        if let Some(s) = info
            .iter()
            .find(|t| t.get("target").and_then(Json::str).and_then(self::target) == Some(target))
            .and_then(|t| t.get("min_deployment"))
            .and_then(Json::str)
        {
            tbd.minos = parse_version(s);
        }
        for group in lib.get("parent_umbrellas").map(Json::arr).unwrap_or(&[]) {
            if applies(group, target) {
                tbd.parent_umbrella = group.get("umbrella").and_then(Json::str);
            }
        }
        for group in lib.get("allowable_clients").map(Json::arr).unwrap_or(&[]) {
            if applies(group, target) {
                tbd.allowable_clients.extend(group.strs("clients"));
            }
        }
        add_symbols(&mut tbd, lib);
        for group in lib
            .get("reexported_libraries")
            .map(Json::arr)
            .unwrap_or(&[])
            .iter()
            .filter(|g| applies(g, target))
        {
            for name in group.strs("names") {
                if !tbd.reexports.contains(&name) {
                    tbd.reexports.push(name);
                }
            }
        }
        Some(tbd)
    };

    let Some(main) = root.get("main_library") else {
        fatal!("{}: no main_library in .tbd file", file.display());
    };
    let mut tbd = parse_library(main)?;
    tbd.platforms = select_target(arch, platform, &targets_of(main)).1;
    // The re-exported libraries inlined in "libraries" are documents
    // of their own; every one counts as re-exported.
    for lib in root.get("libraries").map(Json::arr).unwrap_or(&[]) {
        if let Some(doc) = parse_library(lib) {
            let name: &'static str = String::leak(doc.install_name.clone());
            if !tbd.reexports.contains(&name) {
                tbd.reexports.push(name);
            }
            tbd.documents.push(doc);
        }
    }

    if tbd.install_name.is_empty() {
        fatal!("{}: no install name in .tbd file", file.display());
    }
    Some(tbd)
}

/// Stops the link on a version 5 .tbd TAPI refuses, with its diagnostic
/// naming the section at fault: the first one that isn't what it should
/// be, by the order TAPI reads them in. Of some lists it reads the first
/// element only; unknown keys it ignores.
fn check_json(file: &Path, root: &Json) {
    let fail = |key: &str| -> ! {
        fatal!("tapi error: invalid {key} section\n in '{}'", file.display());
    };
    if root.get("tapi_tbd_version").and_then(Json::integer) != Some(5) {
        fail("tapi_tbd_version");
    }
    let Some(main) = root.get("main_library") else {
        fatal!("{}: no main_library in .tbd file", file.display());
    };
    let libraries = root.get("libraries").map(Json::arr).unwrap_or(&[]);
    for lib in
        std::iter::once(main).chain(libraries.iter().filter(|lib| matches!(lib, Json::Obj(_))))
    {
        check_json_library(lib, &fail);
    }
}

/// check_json for a library: the main one or one inlined.
fn check_json_library(lib: &Json, fail: &dyn Fn(&str) -> !) {
    let Some(Json::Arr(targets)) = lib.get("target_info") else { fail("targets") };
    for info in targets {
        if info.get("target").and_then(Json::str).is_none() {
            fail("target");
        }
        if let Some(version) = info.get("min_deployment").and_then(Json::str)
            && !is_version_tuple(version)
        {
            fail("min_deployment");
        }
    }
    match lib.get("install_names").map(Json::arr).and_then(<[Json]>::first) {
        Some(name @ Json::Obj(_)) if name.get("name").and_then(Json::str).is_none() => fail("name"),
        Some(Json::Obj(_)) => {}
        Some(_) => fail("install_names"),
        None if matches!(lib.get("install_names"), Some(Json::Arr(_))) => {}
        None => fail("install_names"),
    }
    for key in ["current_versions", "compatibility_versions"] {
        match lib.get(key).map(Json::arr).and_then(<[Json]>::first) {
            Some(Json::Obj(_)) => {}
            Some(_) => fail(key),
            None => continue,
        }
        let version = lib.get(key).unwrap().arr()[0].get("version").and_then(Json::str);
        if version.is_some_and(|v| packed_version(v).is_none()) {
            fail("version");
        }
    }
    match lib.get("swift_abi").map(Json::arr).and_then(<[Json]>::first) {
        Some(abi @ Json::Obj(_)) if abi.get("abi").and_then(Json::integer).is_none() => fail("abi"),
        Some(Json::Obj(_)) | None => {}
        Some(_) => fail("swift_abi"),
    }
    match lib.get("flags").map(Json::arr).and_then(<[Json]>::first) {
        Some(flags @ Json::Obj(_)) => check_json_strings(flags, "attributes", fail),
        Some(_) => fail("flags"),
        None => {}
    }
    for umbrella in lib.get("parent_umbrellas").map(Json::arr).unwrap_or(&[]) {
        match umbrella {
            Json::Obj(_) if umbrella.get("umbrella").and_then(Json::str).is_none() => {
                fail("umbrella")
            }
            Json::Obj(_) => {}
            _ => fail("parent_umbrellas"),
        }
    }
    for (key, names) in
        [("allowable_clients", "clients"), ("reexported_libraries", "names"), ("rpaths", "paths")]
    {
        for group in lib.get(key).map(Json::arr).unwrap_or(&[]) {
            check_json_strings(group, names, fail);
        }
    }
    for key in ["exported_symbols", "reexported_symbols", "undefined_symbols"] {
        for group in lib.get(key).map(Json::arr).unwrap_or(&[]) {
            if !matches!(group, Json::Obj(_)) {
                continue;
            }
            let segments: Vec<&Json> = (["data", "text"].iter())
                .filter_map(|seg| group.get(seg).filter(|v| matches!(v, Json::Obj(_))))
                .collect();
            if segments.is_empty() {
                fail(key);
            }
            for segment in segments {
                for kind in
                    ["global", "objc_class", "objc_eh_type", "objc_ivar", "weak", "thread_local"]
                {
                    check_json_strings(segment, kind, fail);
                }
            }
        }
    }
}

/// Fails on an array `key` of `obj` with something other than a string.
fn check_json_strings(obj: &Json, key: &str, fail: &dyn Fn(&str) -> !) {
    if let Some(Json::Arr(items)) = obj.get(key)
        && items.iter().any(|item| item.str().is_none())
    {
        fail(key);
    }
}

/// Whether TAPI reads a version 5 .tbd's min_deployment: one to five
/// numbers, separated by dots.
fn is_version_tuple(s: &str) -> bool {
    let parts = s.split('.');
    let n = parts.clone().count();
    n <= 5 && parts.into_iter().all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

/// A library version as TAPI reads one, packed into 32 bits: up to
/// three dot-separated numbers (an empty one skipped), the first below
/// 65536 and the others below 256.
fn packed_version(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut version = 0;
    for (i, part) in parts.iter().enumerate() {
        if !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let n: u64 = part.parse().ok()?;
        let max = if i == 0 { 0xffff } else { 0xff };
        if n > max {
            return None;
        }
        version |= (n as u32) << (16 - 8 * i);
    }
    Some(version)
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
    let p =
        unsafe { libc::memchr(bytes.as_ptr().add(from).cast(), needle as i32, bytes.len() - from) };
    if p.is_null() { None } else { Some(p as usize - bytes.as_ptr() as usize) }
}

pub fn parse_version(val: &str) -> u32 {
    let mut nums = val.split('.').map(|s| s.parse().unwrap_or(0));
    let major = nums.next().unwrap_or(1);
    let minor = nums.next().unwrap_or(0);
    let patch = nums.next().unwrap_or(0);
    crate::macho::encode_version(major, minor, patch)
}

/// Parses a .tbd file, keeping each of its documents apart.
/// A memoized parse. Stub parsing is pure string work over the mapped
/// file, so results are cached by the file's address and the link's
/// architecture and platform. The big SDK stubs (libSystem's tree,
/// framework umbrellas) can be parsed once, in parallel, by prefetch()
/// before the serial input loop needs them.
pub fn parse_cached(mf: &'static MappedFile, arch: &'static str, platform: u32) -> Option<TbdFile> {
    type Cache = hashbrown::HashMap<(usize, &'static str, u32), Option<TbdFile>>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let key = (mf.data().as_ptr() as usize, arch, platform);
    if let Some(tbd) = CACHE.lock().unwrap().get_or_insert_with(Cache::new).get(&key) {
        return tbd.clone();
    }
    let tbd = parse(mf, arch, platform);
    CACHE.lock().unwrap().get_or_insert_with(Cache::new).insert(key, tbd.clone());
    tbd
}

/// Warms the parse cache on all cores.
pub fn prefetch(
    mfs: &[&'static MappedFile],
    arch: &'static str,
    platform: u32,
) -> Vec<Option<TbdFile>> {
    mfs.par_iter().map(|mf| parse_cached(mf, arch, platform)).collect()
}

/// Parses a .tbd file for the architecture `arch` of a link for
/// `platform`. None if the library has no target on the architecture,
/// which makes ld-prime ignore the file.
pub fn parse(mf: &MappedFile, arch: &'static str, platform: u32) -> Option<TbdFile> {
    let Ok(text): Result<&'static str, _> = std::str::from_utf8(mf.data()) else {
        fatal!("{}: invalid UTF-8 in .tbd file", mf.name.display());
    };

    // TBD version 5 is JSON (tapi's current output, and what Xcode
    // writes for the "eager linking" stubs of frameworks built in the
    // same workspace); versions 1-4 are YAML.
    if text.trim_start().starts_with('{') {
        return parse_json(&mf.name, text, arch, platform);
    }

    // The first document is the library itself; the others are the
    // libraries it re-exports, inlined, each kept as a TbdFile of its
    // own.
    let mut main: Option<TbdFile> = None;
    let mut documents: Vec<TbdFile> = Vec::new();

    let docs = yaml_documents(text);
    check_yaml(mf, text, &docs);

    for (doc, YamlDoc { fields, .. }) in docs.iter().enumerate() {
        let top = || fields.iter().filter(|f| f.indent == 0 && !f.item);
        let (target, platforms) = select_target(arch, platform, &yaml_targets(top()));
        let doc_active = yaml_matches(top(), target);
        if doc == 0 && !doc_active {
            return None;
        }
        if !doc_active {
            continue;
        }
        let mut tbd = TbdFile {
            current_version: crate::macho::encode_version(1, 0, 0),
            compatibility_version: crate::macho::encode_version(1, 0, 0),
            ..TbdFile::default()
        };
        let mut active = doc_active;
        // The top-level key whose value the field is in: the symbols an
        // "undefineds" section lists are the library's imports, which
        // the link has no use for.
        let mut section = "";
        for (i, field) in fields.iter().enumerate() {
            if field.indent == 0 && !field.item {
                active = doc_active;
                section = field.key;
            }
            if field.item {
                let end = fields[i + 1..]
                    .iter()
                    .position(|f| f.indent <= field.indent)
                    .map_or(fields.len(), |n| i + 1 + n);
                active = doc_active && yaml_matches(fields[i..end].iter(), target);
            }
            if field.key == "install-name" {
                tbd.install_name = unquote(field.value).to_string();
            }
            if !active || section == "undefineds" {
                continue;
            }
            match field.key {
                // (check_yaml has made sure they read.)
                "current-version" => {
                    tbd.current_version = packed_version(unquote(field.value)).unwrap()
                }
                "compatibility-version" => {
                    tbd.compatibility_version = packed_version(unquote(field.value)).unwrap()
                }
                // Version 4 lists them per target group ("umbrella:",
                // "clients:"), older versions directly.
                "parent-umbrella" | "umbrella" if !field.value.is_empty() => {
                    tbd.parent_umbrella = Some(unquote(field.value));
                }
                // Version 1 spells it allowed-clients.
                "allowable-clients" | "allowed-clients" | "clients" => {
                    tbd.allowable_clients.extend(field.items())
                }
                "symbols" => tbd.exports.extend(field.items()),
                "weak-symbols" | "weak-def-symbols" => tbd.weak_exports.extend(field.items()),
                "thread-local-symbols" => tbd.tlv_exports.extend(field.items()),
                "libraries" | "re-exports" => {
                    for name in field.items() {
                        if !tbd.reexports.contains(&name) {
                            tbd.reexports.push(name);
                        }
                    }
                }
                "objc-classes" => {
                    for item in field.items() {
                        push_objc_class(&mut tbd.exports, item, false);
                    }
                }
                "objc-eh-types" => {
                    for item in field.items() {
                        push_objc_class(&mut tbd.exports, item, true);
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
        if doc == 0 {
            tbd.platforms = platforms;
            main = Some(tbd);
        } else {
            documents.push(tbd);
        }
    }

    // Every inlined document counts as re-exported, listed or not.
    let mut tbd = main.unwrap_or_default();
    for doc in &documents {
        let name: &'static str = String::leak(doc.install_name.clone());
        if !tbd.reexports.contains(&name) {
            tbd.reexports.push(name);
        }
    }
    tbd.documents = documents;

    if tbd.install_name.is_empty() {
        fatal!("{}: no install-name in .tbd file", mf.name.display());
    }
    Some(tbd)
}

// Retain indentation and list-item boundaries so a target selector applies
// to its whole group, even when it follows the symbols it qualifies.
struct YamlField {
    indent: usize,
    item: bool,
    /// The key, unquoted, and as written.
    key: &'static str,
    raw_key: &'static str,
    /// The value: a scalar, a flow sequence ("[ a, b ]", perhaps over
    /// several lines), the lines of a block sequence of scalars ("- a"
    /// and on) or empty, for a nested mapping or a block sequence of
    /// them in the fields that follow.
    value: &'static str,
}

impl YamlField {
    fn items(&self) -> impl Iterator<Item = &'static str> {
        self.raw_items().map(unquote).filter(|s| !s.is_empty())
    }

    /// The items of a sequence as written, with any blanks after one.
    fn raw_items(&self) -> impl Iterator<Item = &'static str> {
        let block = self.value.starts_with("- ");
        let (body, sep) = match block {
            true => (self.value, '\n'),
            false => (self.value.trim_start_matches('[').trim_end_matches(']'), ','),
        };
        body.split(sep).map(move |item| match block {
            true => item.trim_start().trim_start_matches('-').trim_start(),
            false => item.trim_start(),
        })
    }
}

/// The scalar a block sequence's line ("- a") gives, if it gives one
/// rather than starting a mapping ("- key: value"); a quoted one may
/// have a colon in it ("- 'arm64: <uuid>'").
fn block_scalar(line: &str) -> Option<&str> {
    let item = line.strip_prefix('-')?;
    if !item.is_empty() && !item.starts_with(' ') {
        return None;
    }
    let item = item.trim();
    (item.starts_with(['\'', '"']) || !item.contains(':')).then_some(item)
}

/// Stops the link on a malformed .tbd, `what` is wrong at `item` (a
/// slice of `text`), with ld-prime's YAML reader's diagnostic: the line,
/// and under it the item's first `len` columns marked.
fn malformed(mf: &MappedFile, text: &str, item: &str, len: usize, what: &str) -> ! {
    let off = item.as_ptr() as usize - text.as_ptr() as usize;
    let line_start = text[..off].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[off..].find('\n').map_or(text.len(), |i| off + i);
    let col = off - line_start;
    fatal!(
        "tapi error: malformed file\n{}:{}:{}: error: {what}\n{}\n{}^{}\n in '{}'",
        crate::passes::resolved_file_name(mf),
        text[..off].matches('\n').count() + 1,
        col + 1,
        &text[line_start..line_end],
        " ".repeat(col),
        "~".repeat(len.saturating_sub(1)),
        mf.name.display()
    );
}

/// The columns a YAML scalar spans: a quoted one's as written, a plain
/// one's with the blanks up to the next delimiter on its line.
fn scalar_len(item: &str) -> usize {
    if item.starts_with(['\'', '"']) {
        unquote(item).len() + 2
    } else {
        item.lines().next().unwrap_or("").len()
    }
}

/// Stops the link on a .tbd TAPI's YAML reader refuses, with its
/// diagnostic for the first fault (see malformed). It reads a document
/// by the schema of the version its tag names, key by key in the
/// schema's order, a nested mapping wholly as it comes to one: a key
/// missing that must be there; a value that should be a sequence (or
/// one of mappings) but isn't; a target of a platform TAPI doesn't know
/// (it refuses an architecture it doesn't know too, but those come and
/// go with SDKs - arm64e.x1 - and one unknown here is merely one the
/// link can't use) or a version 1-3 platform it doesn't; and then the
/// first by name of the keys the schema doesn't have. A key given twice
/// in one mapping it refuses before all that, as it reads the document.
fn check_yaml(mf: &MappedFile, text: &str, docs: &[YamlDoc]) {
    for doc in docs.iter().filter(|doc| !doc.fields.is_empty()) {
        let cx = YamlCheck { mf, text, fields: &doc.fields, v4: doc.tag == "!tapi-tbd" };
        cx.check_duplicates(0);
        let schema = match doc.tag {
            "" | "!tapi-tbd-v1" => TOP_V1,
            "!tapi-tbd-v2" => TOP_V2,
            "!tapi-tbd-v3" => TOP_V3,
            "!tapi-tbd" => TOP_V4,
            _ => cx.fail(doc.fields[0].raw_key, 1, "unsupported file type"),
        };
        cx.check_mapping(0, schema);
    }
}

/// A key of a mapping in a .tbd, as TAPI's YAML schema for a version has
/// it: whether it must be there, and what its value is.
struct SchemaKey {
    name: &'static str,
    required: bool,
    value: SchemaValue,
}

#[derive(Clone, Copy)]
enum SchemaValue {
    Scalar,
    Sequence,
    /// A sequence of mappings of these keys.
    Mappings(&'static [SchemaKey]),
}

use SchemaValue::{Mappings, Scalar, Sequence};

const fn key(name: &'static str, value: SchemaValue) -> SchemaKey {
    SchemaKey { name, required: false, value }
}

const fn required(name: &'static str, value: SchemaValue) -> SchemaKey {
    SchemaKey { name, required: true, value }
}

const TOP_V1: &[SchemaKey] = &[
    required("archs", Sequence),
    required("platform", Scalar),
    required("install-name", Scalar),
    key("current-version", Scalar),
    key("compatibility-version", Scalar),
    key("swift-version", Scalar),
    key("objc-constraint", Scalar),
    key("exports", Mappings(EXPORTS_V1)),
];

const EXPORTS_V1: &[SchemaKey] = &[
    required("archs", Sequence),
    key("allowed-clients", Sequence),
    key("re-exports", Sequence),
    key("symbols", Sequence),
    key("objc-classes", Sequence),
    key("objc-ivars", Sequence),
    key("weak-def-symbols", Sequence),
    key("thread-local-symbols", Sequence),
];

const TOP_V2: &[SchemaKey] = &[
    required("archs", Sequence),
    key("uuids", Sequence),
    required("platform", Scalar),
    key("flags", Sequence),
    required("install-name", Scalar),
    key("current-version", Scalar),
    key("compatibility-version", Scalar),
    key("swift-version", Scalar),
    key("objc-constraint", Scalar),
    key("parent-umbrella", Scalar),
    key("exports", Mappings(EXPORTS_V2)),
    key("undefineds", Mappings(UNDEFINEDS_V2)),
];

const EXPORTS_V2: &[SchemaKey] = &[
    required("archs", Sequence),
    key("allowable-clients", Sequence),
    key("re-exports", Sequence),
    key("symbols", Sequence),
    key("objc-classes", Sequence),
    key("objc-ivars", Sequence),
    key("weak-def-symbols", Sequence),
    key("thread-local-symbols", Sequence),
];

const UNDEFINEDS_V2: &[SchemaKey] = &[
    required("archs", Sequence),
    key("symbols", Sequence),
    key("objc-classes", Sequence),
    key("objc-ivars", Sequence),
    key("weak-ref-symbols", Sequence),
];

const TOP_V3: &[SchemaKey] = &[
    required("archs", Sequence),
    key("uuids", Sequence),
    required("platform", Scalar),
    key("flags", Sequence),
    required("install-name", Scalar),
    key("current-version", Scalar),
    key("compatibility-version", Scalar),
    key("swift-abi-version", Scalar),
    key("objc-constraint", Scalar),
    key("parent-umbrella", Scalar),
    key("exports", Mappings(EXPORTS_V3)),
    key("undefineds", Mappings(UNDEFINEDS_V3)),
];

const EXPORTS_V3: &[SchemaKey] = &[
    required("archs", Sequence),
    key("allowable-clients", Sequence),
    key("re-exports", Sequence),
    key("symbols", Sequence),
    key("objc-classes", Sequence),
    key("objc-eh-types", Sequence),
    key("objc-ivars", Sequence),
    key("weak-def-symbols", Sequence),
    key("thread-local-symbols", Sequence),
];

const UNDEFINEDS_V3: &[SchemaKey] = &[
    required("archs", Sequence),
    key("symbols", Sequence),
    key("objc-classes", Sequence),
    key("objc-eh-types", Sequence),
    key("objc-ivars", Sequence),
    key("weak-ref-symbols", Sequence),
];

const TOP_V4: &[SchemaKey] = &[
    required("tbd-version", Scalar),
    required("targets", Sequence),
    key("uuids", Mappings(&[required("target", Scalar), required("value", Scalar)])),
    key("flags", Sequence),
    required("install-name", Scalar),
    key("current-version", Scalar),
    key("compatibility-version", Scalar),
    key("swift-abi-version", Scalar),
    key(
        "parent-umbrella",
        Mappings(&[required("targets", Sequence), required("umbrella", Scalar)]),
    ),
    key(
        "allowable-clients",
        Mappings(&[required("targets", Sequence), required("clients", Sequence)]),
    ),
    key(
        "reexported-libraries",
        Mappings(&[required("targets", Sequence), required("libraries", Sequence)]),
    ),
    key("exports", Mappings(SYMBOLS_V4)),
    key("reexports", Mappings(SYMBOLS_V4)),
    key("undefineds", Mappings(SYMBOLS_V4)),
];

const SYMBOLS_V4: &[SchemaKey] = &[
    required("targets", Sequence),
    key("symbols", Sequence),
    key("objc-classes", Sequence),
    key("objc-eh-types", Sequence),
    key("objc-ivars", Sequence),
    key("weak-symbols", Sequence),
    key("thread-local-symbols", Sequence),
];

/// A YAML document's fields under check_yaml. A mapping is known by
/// the index of its first key.
struct YamlCheck<'a> {
    mf: &'a MappedFile,
    text: &'a str,
    fields: &'a [YamlField],
    /// The document is in version 4 of the format.
    v4: bool,
}

impl YamlCheck<'_> {
    fn fail(&self, item: &str, len: usize, what: &str) -> ! {
        malformed(self.mf, self.text, item, len, what)
    }

    /// The column the key at `i` starts at: an item's after its "- ".
    fn column(&self, i: usize) -> usize {
        self.fields[i].indent + if self.fields[i].item { 2 } else { 0 }
    }

    /// Where the fields of the value of the key at `i` end: those of a
    /// mapping deeper than it, or of a block sequence's items, whose
    /// "- " may start at its own column.
    fn value_end(&self, i: usize) -> usize {
        let col = self.column(i);
        let rest = &self.fields[i + 1..];
        i + 1 + rest.iter().take_while(|f| f.indent > col || (f.item && f.indent == col)).count()
    }

    /// The keys of a mapping.
    fn mapping_keys(&self, first: usize) -> Vec<usize> {
        let col = self.column(first);
        let mut keys = vec![first];
        let mut i = self.value_end(first);
        while i < self.fields.len() && !self.fields[i].item && self.fields[i].indent == col {
            keys.push(i);
            i = self.value_end(i);
        }
        keys
    }

    /// The mappings the key at `i` has for its value: a block sequence's
    /// or a nested one.
    fn nested_mappings(&self, i: usize) -> Vec<usize> {
        let end = self.value_end(i);
        if i + 1 == end {
            return Vec::new();
        }
        let first = &self.fields[i + 1];
        if !first.item {
            return vec![i + 1];
        }
        (i + 1..end)
            .filter(|&j| self.fields[j].item && self.fields[j].indent == first.indent)
            .collect()
    }

    fn check_duplicates(&self, first: usize) {
        let keys = self.mapping_keys(first);
        for (n, &i) in keys.iter().enumerate() {
            let field = &self.fields[i];
            if keys[..n].iter().any(|&j| self.fields[j].key == field.key) {
                let what = format!("duplicated mapping key '{}'", field.key);
                self.fail(field.raw_key, field.raw_key.len(), &what);
            }
            for nested in self.nested_mappings(i) {
                self.check_duplicates(nested);
            }
        }
    }

    fn check_mapping(&self, first: usize, schema: &[SchemaKey]) {
        let keys = self.mapping_keys(first);
        for key in schema {
            match keys.iter().find(|&&i| self.fields[i].key == key.name) {
                Some(&i) => self.check_value(i, key),
                None if key.required => {
                    let what = format!("missing required key '{}'", key.name);
                    self.fail(self.fields[first].raw_key, 1, &what);
                }
                None => {}
            }
        }
        let unknown = (keys.iter().map(|&i| &self.fields[i]))
            .filter(|f| !schema.iter().any(|key| key.name == f.key))
            .min_by_key(|f| f.key);
        if let Some(f) = unknown {
            self.fail(f.raw_key, f.raw_key.len(), &format!("unknown key '{}'", f.key));
        }
    }

    fn check_value(&self, i: usize, key: &SchemaKey) {
        let field = &self.fields[i];
        let value = field.value;
        let is_sequence = value.starts_with('[') || value.starts_with("- ");
        // A null is an empty sequence, but for a set of flags.
        let is_null = value.is_empty() || matches!(value, "~" | "null" | "Null" | "NULL");
        let is_scalar = !is_sequence && !is_null;
        match key.value {
            Scalar => self.check_scalar(key.name, value),
            Sequence if key.name == "flags" => {
                if !is_sequence && !value.is_empty() {
                    self.fail(value, scalar_len(value), "expected sequence of bit values");
                }
                let known = |item: &str| {
                    let flags = ["flat_namespace", "not_app_extension_safe", "installapi"];
                    flags.contains(&item) || item == "not_for_dyld_shared_cache"
                };
                let unknown = |item: &&str| !item.is_empty() && !known(unquote(item));
                if let Some(item) = field.raw_items().find(unknown) {
                    self.fail(item, scalar_len(item), "unknown bit value");
                }
            }
            Sequence => {
                if is_scalar {
                    self.fail(value, scalar_len(value), "not a sequence");
                }
                let unknown =
                    |item: &&'static str| !item.is_empty() && target(unquote(item)).is_none();
                if key.name == "targets"
                    && let Some(item) = field.raw_items().find(unknown)
                {
                    self.fail(item, scalar_len(item), "unknown target");
                }
            }
            Mappings(schema) => {
                if is_scalar {
                    self.fail(value, scalar_len(value), "not a sequence");
                }
                if let Some(item) = field.raw_items().find(|item| !item.is_empty()) {
                    self.fail(item, scalar_len(item), "not a mapping");
                }
                for nested in self.nested_mappings(i) {
                    self.check_mapping(nested, schema);
                }
            }
        }
    }

    /// Checks a scalar's value as the type TAPI reads it as takes it. In
    /// place of a scalar, a sequence fails it with the same complaint,
    /// at the sequence's first token (or "unexpected scalar", for a
    /// plain string); an empty value fails it at the token that follows.
    fn check_scalar(&self, key: &str, value: &'static str) {
        let what = match key {
            "platform" => "unknown platform",
            "current-version" | "compatibility-version" => "invalid packed version string.",
            "swift-version" | "swift-abi-version" => "invalid Swift ABI version.",
            "objc-constraint" => "unknown enumerated scalar",
            "tbd-version" => "invalid number",
            "target" => "unknown target",
            _ => "unexpected scalar",
        };
        if let Some(seq) = value.strip_prefix('[').or_else(|| value.strip_prefix("- ")) {
            self.fail(seq.trim_start(), 1, what);
        }
        let v = unquote(value);
        let ok = match key {
            "platform" => !legacy_platforms(v).is_empty(),
            "current-version" | "compatibility-version" => packed_version(v).is_some(),
            "swift-version" | "swift-abi-version" => {
                (!self.v4 && matches!(v, "1.0" | "1.1" | "2.0" | "3.0"))
                    || (v.bytes().all(|c| c.is_ascii_digit()) && v.parse::<u8>().is_ok())
            }
            "objc-constraint" => matches!(
                v,
                "none"
                    | "retain_release"
                    | "retain_release_for_simulator"
                    | "retain_release_or_gc"
                    | "gc"
            ),
            "tbd-version" => match auto_radix_number(v) {
                Some(n) if n > u32::MAX as u64 => {
                    self.fail(value, scalar_len(value), "out of range number")
                }
                n => n.is_some(),
            },
            "target" => target(v).is_some(),
            _ => true,
        };
        if !ok {
            let off = value.as_ptr() as usize - self.text.as_ptr() as usize;
            let at = if value.is_empty() { self.text[off..].trim_start() } else { value };
            self.fail(at, scalar_len(value), what);
        }
    }
}

/// A number as LLVM reads one with its radix from its prefix: 0x for
/// hexadecimal, 0b binary, 0o or 0 octal, and decimal otherwise.
fn auto_radix_number(s: &str) -> Option<u64> {
    let (digits, radix) = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        (hex, 16)
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        (bin, 2)
    } else if let Some(oct) = s.strip_prefix("0o") {
        (oct, 8)
    } else if s.len() > 1 && s.starts_with('0') {
        (&s[1..], 8)
    } else {
        (s, 10)
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    u64::from_str_radix(digits, radix).ok()
}

/// The targets of a document's top-level fields: a version 4 file's, or
/// a version 1-3 file's architectures on its platform.
fn yaml_targets<'a>(fields: impl Iterator<Item = &'a YamlField> + Clone) -> Vec<Target> {
    let platforms = fields
        .clone()
        .find(|f| f.key == "platform")
        .map_or(&[PLATFORM_MACOS][..], |f| legacy_platforms(unquote(f.value)));
    fields
        .flat_map(|f| match f.key {
            "targets" => f.items().filter_map(target).collect(),
            "archs" => f.items().flat_map(|a| platforms.iter().map(move |&p| (a, p))).collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn yaml_matches<'a>(fields: impl Iterator<Item = &'a YamlField>, want: Target) -> bool {
    fields.into_iter().all(|field| match field.key {
        "targets" => field.items().any(|s| target(s) == Some(want)),
        "archs" => field.items().any(|s| s == want.0),
        "platform" => legacy_platforms(unquote(field.value)).contains(&want.1),
        _ => true,
    })
}

/// A YAML document of a .tbd: the tag its "---" line gives it (which
/// says the version of the format), and its fields.
struct YamlDoc {
    tag: &'static str,
    fields: Vec<YamlField>,
}

fn yaml_documents(text: &'static str) -> Vec<YamlDoc> {
    let bytes = text.as_bytes();
    let mut docs = vec![YamlDoc { tag: "", fields: Vec::new() }];
    let mut pos = 0;
    while pos < bytes.len() {
        let eol = memchr_from(bytes, b'\n', pos).unwrap_or(bytes.len());
        let raw = &text[pos..eol];
        let line = raw.trim_start();
        let mut next = eol + 1;
        if let Some(tag) = line.strip_prefix("---") {
            if !docs.last().unwrap().fields.is_empty() {
                docs.push(YamlDoc { tag: "", fields: Vec::new() });
            }
            docs.last_mut().unwrap().tag = tag.trim();
        }
        let fields = &mut docs.last_mut().unwrap().fields;
        if line.starts_with("---") || line.starts_with('#') {
        } else if let Some(item) = block_scalar(line) {
            // A block sequence's scalars are its key's value, from the
            // first one to the last.
            if let Some(last) = fields.last_mut()
                && (last.value.is_empty() || last.value.starts_with("- "))
                && !item.is_empty()
            {
                let start = match last.value.is_empty() {
                    true => line.as_ptr() as usize - text.as_ptr() as usize,
                    false => last.value.as_ptr() as usize - text.as_ptr() as usize,
                };
                let end = item.as_ptr() as usize - text.as_ptr() as usize + item.len();
                last.value = &text[start..end];
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
            let raw_key = key.trim_end();
            fields.push(YamlField {
                indent: raw.len() - line.len(),
                item: line.starts_with("- "),
                key: unquote(raw_key),
                raw_key,
                value,
            });
        }
        pos = next;
    }
    docs
}

/// A .tbd target: an architecture and a platform.
type Target = (&'static str, u32);

/// The platforms of .tbd targets by the names they go by after the
/// architecture, as in "arm64-macos" or "x86_64-ios-simulator".
const TARGET_PLATFORMS: [(&str, u32); 14] = [
    ("macos", PLATFORM_MACOS),
    ("ios", PLATFORM_IOS),
    ("tvos", PLATFORM_TVOS),
    ("watchos", PLATFORM_WATCHOS),
    ("bridgeos", PLATFORM_BRIDGEOS),
    ("maccatalyst", PLATFORM_MACCATALYST),
    ("ios-simulator", PLATFORM_IOSSIMULATOR),
    ("tvos-simulator", PLATFORM_TVOSSIMULATOR),
    ("watchos-simulator", PLATFORM_WATCHOSSIMULATOR),
    ("driverkit", PLATFORM_DRIVERKIT),
    ("xros", PLATFORM_VISIONOS),
    ("xros-simulator", PLATFORM_VISIONOSSIMULATOR),
    ("firmware", PLATFORM_FIRMWARE),
    ("sepos", PLATFORM_SEPOS),
];

/// Parses a target such as "arm64-macos"; None for a platform unknown.
fn target(s: &'static str) -> Option<Target> {
    let (arch, name) = s.split_once('-')?;
    TARGET_PLATFORMS.iter().find(|&&(n, _)| n == name).map(|&(_, p)| (arch, p))
}

/// The platforms a version 1-3 file's "platform" names; "zippered" is
/// macOS and Mac Catalyst both.
fn legacy_platforms(name: &str) -> &'static [u32] {
    match name {
        "macosx" => &[PLATFORM_MACOS],
        "ios" => &[PLATFORM_IOS],
        "tvos" => &[PLATFORM_TVOS],
        "watchos" => &[PLATFORM_WATCHOS],
        "bridgeos" => &[PLATFORM_BRIDGEOS],
        "iosmac" | "maccatalyst" => &[PLATFORM_MACCATALYST],
        "zippered" => &[PLATFORM_MACOS, PLATFORM_MACCATALYST],
        "driverkit" => &[PLATFORM_DRIVERKIT],
        _ => &[],
    }
}

/// The target a library is read for, of `targets` its own: the
/// architecture (see select_arch) on the link's platform if the library
/// has that, else on its first. Also returns the platforms it has for
/// the architecture, in platform order.
fn select_target(arch: &'static str, platform: u32, targets: &[Target]) -> (Target, Vec<u32>) {
    let arch = select_arch(arch, targets.iter().map(|&(a, _)| a));
    let mut platforms: Vec<u32> =
        targets.iter().filter(|&&(a, _)| a == arch).map(|&(_, p)| p).collect();
    platforms.sort_unstable();
    platforms.dedup();
    let chosen =
        if platforms.is_empty() || platforms.contains(&platform) { platform } else { platforms[0] };
    ((arch, chosen), platforms)
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
        Box::leak(Box::new(MappedFile {
            name: std::path::PathBuf::from("test.tbd"),
            data: text.as_bytes(),
            parent: None,
            mtime: None,
        }))
    }

    #[test]
    fn yaml_target_groups_and_cache() {
        let mf = mapped(
            r#"--- !tapi-tbd
tbd-version: 4
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
        let arm = parse_cached(mf, "arm64", PLATFORM_MACOS).unwrap();
        assert_eq!(arm.exports, ["_arm", "_OBJC_CLASS_$_Arm", "_OBJC_METACLASS_$_Arm"]);
        assert_eq!(arm.weak_exports, ["_weak_arm"]);
        assert_eq!(arm.tlv_exports, ["_tls_arm"]);
        assert_eq!(arm.reexports, ["/arm"]);
        let x86 = parse_cached(mf, "x86_64", PLATFORM_MACOS).unwrap();
        assert_eq!(x86.exports, ["_x86"]);
        assert!(x86.weak_exports.is_empty());
        assert!(x86.tlv_exports.is_empty());
        assert!(x86.reexports.is_empty());
    }

    #[test]
    fn arm64e_fallback_prefers_exact_architecture() {
        let mf = mapped(
            r#"--- !tapi-tbd
tbd-version: 4
targets: [ arm64-macos, arm64e-macos ]
install-name: /libtest
exports:
  - targets: [ arm64-macos ]
    symbols: [ _arm ]
  - targets: [ arm64e-macos ]
    symbols: [ _arme ]
--- !tapi-tbd
tbd-version: 4
targets: [ arm64e-macos ]
install-name: /inline
exports:
  - targets: [ arm64e-macos ]
    symbols: [ _fallback ]
"#,
        );
        let tbd = parse(mf, "arm64", PLATFORM_MACOS).unwrap();
        assert_eq!(tbd.exports, ["_arm"]);
        assert_eq!(tbd.reexports, ["/inline"]);
        assert_eq!(tbd.document("/inline").unwrap().exports, ["_fallback"]);
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
        assert_eq!(parse(mf, "arm64", PLATFORM_MACOS).unwrap().exports, ["_arm"]);
        assert_eq!(parse(mf, "x86_64", PLATFORM_MACOS).unwrap().exports, ["_x86"]);
    }

    #[test]
    fn json_target_groups_and_inline_libraries() {
        let mf = mapped(
            r#"{"tapi_tbd_version":5,"main_library":{
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
        let arm = parse(mf, "arm64", PLATFORM_MACOS).unwrap();
        assert_eq!(arm.exports, ["_both"]);
        assert_eq!(arm.weak_exports, ["_weak"]);
        assert_eq!(arm.tlv_exports, ["_tls"]);
        assert_eq!(arm.reexports, ["/arm", "/inline"]);
        assert_eq!(arm.document("/inline").unwrap().exports, ["_inline"]);
        let x86 = parse(mf, "x86_64", PLATFORM_MACOS).unwrap();
        assert_eq!(x86.exports, ["_both"]);
        assert!(x86.weak_exports.is_empty());
        assert!(x86.tlv_exports.is_empty());
        assert_eq!(x86.reexports, ["/x86"]);
        assert!(x86.documents.is_empty());
    }
}
