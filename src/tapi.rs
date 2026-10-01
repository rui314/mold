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
    Num,
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

    let targets_of = |lib: &Json| -> Vec<Target> {
        let info = lib.get("target_info").map(Json::arr).unwrap_or(&[]);
        info.iter().filter_map(|t| t.get("target").and_then(Json::str)).filter_map(target).collect()
    };
    let target_of = |lib: &Json| select_target(arch, platform, &targets_of(lib)).0;
    let applies = |group: &Json, want: Target| {
        group.get("targets").is_none() || group.strs("targets").any(|t| target(t) == Some(want))
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
        if let Some(name) = lib
            .get("install_names")
            .map(Json::arr)
            .and_then(|a| a.iter().find(|g| applies(g, target)))
            && let Some(s) = name.get("name").and_then(Json::str)
        {
            tbd.install_name = s.to_string();
        }
        if let Some(v) = lib
            .get("current_versions")
            .map(Json::arr)
            .and_then(|a| a.iter().find(|g| applies(g, target)))
            && let Some(s) = v.get("version").and_then(Json::str)
        {
            tbd.current_version = parse_version(s);
        }
        if let Some(v) = lib
            .get("compatibility_versions")
            .map(Json::arr)
            .and_then(|a| a.iter().find(|g| applies(g, target)))
            && let Some(s) = v.get("version").and_then(Json::str)
        {
            tbd.compatibility_version = parse_version(s);
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

    for (doc, fields) in docs.iter().enumerate() {
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
        for (i, field) in fields.iter().enumerate() {
            if field.indent == 0 && !field.item {
                active = doc_active;
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
            if !active {
                continue;
            }
            match field.key {
                "current-version" => tbd.current_version = parse_version(unquote(field.value)),
                "compatibility-version" => {
                    tbd.compatibility_version = parse_version(unquote(field.value))
                }
                // Version 4 lists them per target group ("umbrella:",
                // "clients:"), older versions directly.
                "parent-umbrella" | "umbrella" if !field.value.is_empty() => {
                    tbd.parent_umbrella = Some(unquote(field.value));
                }
                "allowable-clients" | "clients" => tbd.allowable_clients.extend(field.items()),
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
    key: &'static str,
    value: &'static str,
}

impl YamlField {
    fn items(&self) -> impl Iterator<Item = &'static str> {
        self.raw_items().map(unquote).filter(|s| !s.is_empty())
    }

    /// The items of a flow list as written, with any blanks after one.
    fn raw_items(&self) -> impl Iterator<Item = &'static str> {
        self.value.trim_start_matches('[').trim_end_matches(']').split(',').map(str::trim_start)
    }
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

/// Stops the link on a .tbd TAPI refuses: one naming a target of a
/// platform it doesn't know anywhere (it refuses an architecture it
/// doesn't know too, but those come and go with SDKs - arm64e.x1 - and
/// one unknown here is merely one the link can't use), or a version 1-3
/// document without a platform of those it knows.
fn check_yaml(mf: &MappedFile, text: &str, docs: &[Vec<YamlField>]) {
    for field in docs.iter().flatten().filter(|f| f.key == "targets") {
        if let Some(item) =
            field.raw_items().find(|&item| !item.is_empty() && target(unquote(item)).is_none())
        {
            malformed(mf, text, item, scalar_len(item), "unknown target");
        }
    }
    for fields in docs {
        let top = || fields.iter().filter(|f| f.indent == 0 && !f.item);
        if !top().any(|f| f.key == "archs") {
            continue;
        }
        match top().find(|f| f.key == "platform") {
            Some(f) if !legacy_platforms(unquote(f.value)).is_empty() => {}
            Some(f) => malformed(mf, text, f.value, scalar_len(f.value), "unknown platform"),
            None => malformed(mf, text, fields[0].key, 1, "missing required key 'platform'"),
        }
    }
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
