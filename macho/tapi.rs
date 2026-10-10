//! This file reads text-based dylib stubs (.tbd files), which describe a
//! dylib in place of the dylib itself.
//!
//! Apple's SDKs don't contain the system's dylibs. To link against a
//! dylib, the linker only needs to know what it exports, so the SDK has a
//! text file for each dylib, made by Apple's TAPI tool, that gives its
//! install name (the path it is loaded by, like an ELF soname), versions,
//! exported symbols and re-exported libraries. The linker reads the file
//! in place of the dylib, much as it would read the dylib's symbol table.
//! Versions 1 to 4 of the format are YAML, and version 5 is JSON. We don't
//! need a general YAML parser: the files are machine-generated and regular,
//! so a line-oriented scan is enough.
//!
//! A .tbd file may contain several documents: the first one is the library
//! itself, and the rest are the libraries it re-exports, inlined. Each
//! document is kept apart, because the linker decides per library whether
//! its symbols resolve through the top-level library or bind to the
//! re-exported library directly.
//!
//! A library's exports may also include names starting with "$ld$", which
//! aren't symbols but directives to the linker that change the library for
//! some deployment targets, e.g. to make a link for an older OS bind a
//! symbol to the library where it used to live (see LdSymbols).

use std::path::Path;

use mold_common::fatal;
use serde_json::Value;

use crate::context::Context;
use crate::macho::*;
use crate::mapped_file::MappedFile;

/// A stub's names are text, as TAPI reads only UTF-8, but the linker
/// takes them as the bytes they are, as it does any other name.
#[derive(Debug, Default, Clone)]
pub struct TbdFile {
    pub install_name: &'static [u8],
    pub current_version: u32,
    /// The compatibility version (1.0.0 when the stub gives none), for
    /// the client's LC_LOAD_DYLIB.
    pub compatibility_version: u32,
    pub exports: Vec<&'static [u8]>,
    pub weak_exports: Vec<&'static [u8]>,
    /// Exports that are thread-local variables (listed separately in
    /// .tbd files; a TLV can only be referenced through TLV
    /// relocations).
    pub tlv_exports: Vec<&'static [u8]>,
    /// The exports that are linker directives ("$ld$..."), in order,
    /// found as the file is parsed (see find_ld_symbols).
    pub ld_symbols: Vec<&'static [u8]>,
    /// The umbrella the library belongs to (parent-umbrella), and the
    /// clients it lets link it directly (allowable-clients).
    pub parent_umbrella: Option<&'static [u8]>,
    pub allowable_clients: Vec<&'static [u8]>,
    /// Install names of the libraries this one re-exports: documents
    /// inlined in the same file and libraries in files of their own
    /// alike. An inlined document no document lists is not one: ld-prime
    /// leaves its symbols undefined.
    pub reexports: Vec<&'static [u8]>,
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

#[cfg(test)]
impl TbdFile {
    /// The inlined document for a re-exported library, by install name.
    pub fn document(&self, install_name: &[u8]) -> Option<&Self> {
        self.documents.iter().find(|d| d.install_name == install_name)
    }
}

/// The elements of an object's array-valued key; none for a key with
/// another value, or none.
fn list<'a>(obj: &'a Value, key: &str) -> &'a [Value] {
    obj.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// The strings of an object's array-valued key.
fn strs(obj: &'static Value, key: &str) -> impl Iterator<Item = &'static str> {
    list(obj, key).iter().filter_map(Value::as_str)
}

/// Adds the symbols of an Objective-C class a stub lists by name to its
/// exports: the class and metaclass objects, and with `eh_type` the
/// exception type too. tapi has a class listed for its exception type
/// alone (objc-eh-types, objc_eh_type) export all three: ld-prime links
/// _OBJC_CLASS_$_Foo against such a stub.
fn push_objc_class(exports: &mut Vec<&'static [u8]>, name: &str, eh_type: bool) {
    exports.push(leak_name("_OBJC_CLASS_$_", name));
    exports.push(leak_name("_OBJC_METACLASS_$_", name));
    if eh_type {
        exports.push(leak_name("_OBJC_EHTYPE_$_", name));
    }
}

/// A symbol name a stub spells as a prefix and a name, which the link
/// keeps to its end.
fn leak_name(prefix: &str, name: &str) -> &'static [u8] {
    mold_common::mem::leak_bytes([prefix.as_bytes(), name.as_bytes()].concat())
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
    // The parsed file lives as long as the link, like the input files,
    // so that the names in it can be borrowed.
    let root: &'static Value = match serde_json::from_str(text) {
        Ok(root) => Box::leak(Box::new(root)),
        Err(e) => fatal!("{}: malformed .tbd JSON: {e}", file.display()),
    };
    if root["tapi_tbd_version"] != 5 {
        fatal!("{}: unsupported .tbd version", file.display());
    }

    let Some(main) = root.get("main_library") else {
        fatal!("{}: no main_library in .tbd file", file.display());
    };
    let mut tbd = parse_json_library(main, arch, platform)?;
    tbd.platforms = select_target(arch, platform, &json_targets(main)).1;
    // The re-exported libraries inlined in "libraries" are documents
    // of their own.
    let libs = list(root, "libraries").iter();
    tbd.documents = libs.filter_map(|lib| parse_json_library(lib, arch, platform)).collect();

    if tbd.install_name.is_empty() {
        fatal!("{}: no install name in .tbd file", file.display());
    }
    Some(tbd)
}

/// The targets a library object of a TBD v5 file has.
fn json_targets(lib: &'static Value) -> Vec<Target> {
    let info = list(lib, "target_info").iter();
    info.filter_map(|t| t.get("target").and_then(Value::as_str)).filter_map(target).collect()
}

/// Whether a group of a library object's list applies to `want`: a
/// group without targets applies to all of the library's.
fn json_applies(group: &'static Value, want: Target) -> bool {
    group.get("targets").is_none() || strs(group, "targets").any(|t| target(t) == Some(want))
}

/// One library object of a TBD v5 file (the main library or an inlined
/// re-export) as a TbdFile: its install name, version, flags, symbols
/// and the names it re-exports, read for the architecture `arch` of a
/// link for `platform`. None if it has no target on the architecture.
fn parse_json_library(lib: &'static Value, arch: &'static str, platform: u32) -> Option<TbdFile> {
    let targets = json_targets(lib);
    let target = select_target(arch, platform, &targets).0;
    if !targets.contains(&target) {
        return None;
    }
    let mut tbd = TbdFile {
        current_version: crate::macho::encode_version(1, 0, 0),
        compatibility_version: crate::macho::encode_version(1, 0, 0),
        ..TbdFile::default()
    };
    // TAPI takes the install name and versions from the first entry
    // of their lists, whatever its targets.
    let first = |key: &str, field: &str| list(lib, key).first()?.get(field)?.as_str();
    if let Some(s) = first("install_names", "name") {
        tbd.install_name = s.as_bytes();
    }
    if let Some(s) = first("current_versions", "version") {
        tbd.current_version = parse_version(s);
    }
    if let Some(s) = first("compatibility_versions", "version") {
        tbd.compatibility_version = parse_version(s);
    }
    if let Some(s) = (list(lib, "target_info").iter())
        .find(|t| t["target"].as_str().and_then(self::target) == Some(target))
        .and_then(|t| t.get("min_deployment"))
        .and_then(Value::as_str)
    {
        tbd.minos = parse_version(s);
    }
    let groups = |key| list(lib, key).iter().filter(move |g| json_applies(g, target));
    for group in groups("parent_umbrellas") {
        tbd.parent_umbrella = group.get("umbrella").and_then(Value::as_str).map(str::as_bytes);
    }
    for group in groups("allowable_clients") {
        tbd.allowable_clients.extend(strs(group, "clients").map(str::as_bytes));
    }
    for group in groups("exported_symbols").chain(groups("reexported_symbols")) {
        add_json_symbols(&mut tbd, group);
    }
    for group in groups("reexported_libraries") {
        for name in strs(group, "names").map(str::as_bytes) {
            if !tbd.reexports.contains(&name) {
                tbd.reexports.push(name);
            }
        }
    }
    Some(tbd)
}

/// Adds the symbols a group of a library object's exported or
/// re-exported symbols lists, in its "data" and "text" sections.
fn add_json_symbols(tbd: &mut TbdFile, group: &'static Value) {
    for section in ["data", "text"] {
        let Some(kinds) = group.get(section) else { continue };
        tbd.exports.extend(strs(kinds, "global").map(str::as_bytes));
        tbd.weak_exports.extend(strs(kinds, "weak").map(str::as_bytes));
        tbd.tlv_exports.extend(strs(kinds, "thread_local").map(str::as_bytes));
        for name in strs(kinds, "objc_class") {
            push_objc_class(&mut tbd.exports, name, false);
        }
        for name in strs(kinds, "objc_eh_type") {
            push_objc_class(&mut tbd.exports, name, true);
        }
        for name in strs(kinds, "objc_ivar") {
            tbd.exports.push(leak_name("_OBJC_IVAR_$_", name));
        }
    }
}

/// Strips a YAML scalar's surrounding quotes, if any.
fn unquote(s: &str) -> &str {
    let s = trim_end(trim_start(s));
    s.strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .or_else(|| s.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
        .unwrap_or(s)
}

/// The position of the first `needle` in `bytes` at or after `from`.
fn memchr_from(bytes: &[u8], needle: u8, from: usize) -> Option<usize> {
    memchr::memchr(needle, bytes.get(from..)?).map(|i| from + i)
}

fn parse_version(val: &str) -> u32 {
    let mut nums = val.split('.').map(|s| s.parse().unwrap_or(0));
    let major = nums.next().unwrap_or(1);
    let minor = nums.next().unwrap_or(0);
    let patch = nums.next().unwrap_or(0);
    crate::macho::encode_version(major, minor, patch)
}

/// Parses a .tbd file as `parse` does, memoized. Stub parsing is pure
/// string work over the mapped file, so results are cached by the
/// file's address and the link's architecture and platform.
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

/// Parses a .tbd file for the architecture `arch` of a link for
/// `platform`, keeping each of its libraries apart. None if the library
/// has no target on the architecture, which makes ld-prime ignore the
/// file.
pub fn parse(mf: &MappedFile, arch: &'static str, platform: u32) -> Option<TbdFile> {
    let Ok(text): Result<&'static str, _> = std::str::from_utf8(mf.data()) else {
        fatal!("{}: invalid UTF-8 in .tbd file", mf.name.display());
    };
    // TBD version 5 is JSON (tapi's current output, and what Xcode
    // writes for the "eager linking" stubs of frameworks built in the
    // same workspace); versions 1-4 are YAML.
    let mut tbd = if text.trim_start().starts_with('{') {
        parse_json(&mf.name, text, arch, platform)
    } else {
        parse_yaml(&mf.name, text, arch, platform)
    }?;
    find_ld_symbols(&mut tbd);
    tbd.documents.iter_mut().for_each(find_ld_symbols);
    Some(tbd)
}

/// Notes a library's linker directives among its exports, which the
/// link reads before it takes the library (see
/// interpret_ld_symbols): an SDK framework's stub has tens
/// of thousands of exports, few or none of them directives.
fn find_ld_symbols(tbd: &mut TbdFile) {
    tbd.ld_symbols = tbd.exports.iter().copied().filter(|n| n.starts_with(b"$ld$")).collect();
}

/// An export that a per-symbol $ld$previous directive moves to an older
/// library for the link's target: it binds to the library with that
/// install name, at the directive's version or else the defining
/// library's.
#[derive(Clone)]
pub struct MovedExport {
    pub name: &'static [u8],
    pub install_name: &'static [u8],
    pub current_version: u32,
    pub compatibility_version: u32,
}

/// What a library's "$ld$..." names say for the link's target beyond its
/// exports: whether its install name is an older library's, and the
/// exports that move to one.
#[derive(Clone)]
pub struct LdDirectives {
    pub renamed: bool,
    pub moved: Vec<MovedExport>,
}

/// A library's "$ld$..." names, read for the link's target. These are
/// not symbols but directives to the linker, invented so a library could
/// change shape per deployment target without a file format change:
/// $ld$add$os<ver>$<sym> exports <sym> only when the target equals
/// <ver>, $ld$hide$os<ver>$<sym> hides one, $ld$install_name$os<ver>$
/// <name> substitutes the recorded install name,
/// $ld$compatibility_version$os<ver>$<version> the compatibility
/// version, and $ld$previous$<name>$<compat>$<platform>$<lo>$<hi>$<sym>$
/// applies <name> (at version <compat>, if given) when the target
/// platform matches and lo <= minos < hi: to the whole library if <sym>
/// is empty, else to that export alone. Apple uses these when symbols
/// move between libraries: old targets keep binding them where they
/// used to live (AppKit's Swift overlay functions in libswiftAppKit
/// before macOS 14). A stub lists them among its exports, and a binary
/// dylib exports them as absolute symbols; ld-prime obeys both, and
/// passes over one it can't read without a word. Of several directives
/// of a kind that apply, it takes the one with the first $ld$previous
/// install name, the last $ld$install_name one and the first
/// $ld$compatibility_version directive by name; a library's
/// $ld$previous beats its $ld$install_name.
pub struct LdSymbols {
    pub added: Vec<&'static [u8]>,
    hidden: hashbrown::HashSet<&'static [u8]>,
    /// The install name an $ld$install_name directive gives.
    install_name: Option<&'static [u8]>,
    /// The install name a whole-library $ld$previous directive gives,
    /// with the version it gives, if any.
    previous: Option<(&'static [u8], Option<u32>)>,
    /// The $ld$compatibility_version directive that applies: its name
    /// and version.
    compatibility_version: Option<(&'static [u8], u32)>,
    /// The exports that move: each with the install name it moves to
    /// and the version the directive gives, if any.
    moved: Vec<(&'static [u8], &'static [u8], Option<u32>)>,
}

impl LdSymbols {
    /// Reads the directives among `names`, which may hold other names.
    pub fn read<E: crate::arch::Target>(ctx: &Context<E>, names: &[&'static [u8]]) -> Self {
        use hashbrown::hash_map::Entry;
        let minos = ctx.args.platform_minos;
        let mut ld = Self {
            added: Vec::new(),
            hidden: hashbrown::HashSet::new(),
            install_name: None,
            previous: None,
            compatibility_version: None,
            moved: Vec::new(),
        };
        // Where each moved export is in `ld.moved`: SwiftUICore moves
        // some 15,000 for a macOS 13 target.
        let mut moved_at: hashbrown::HashMap<&[u8], usize> = hashbrown::HashMap::new();
        for &name in names {
            let Some(rest) = name.strip_prefix(b"$ld$") else { continue };
            if let Some(rest) = rest.strip_prefix(b"previous$") {
                let Some(p) = PreviousDirective::parse(rest, ctx.args.platform) else { continue };
                if minos < p.lo || p.hi <= minos {
                    continue;
                }
                if p.sym.is_empty() {
                    if ld.previous.is_none_or(|(first, _)| p.install_name < first) {
                        ld.previous = Some((p.install_name, p.version));
                    }
                    continue;
                }
                let moved = (p.sym, p.install_name, p.version);
                match moved_at.entry(p.sym) {
                    Entry::Occupied(e) if p.install_name < ld.moved[*e.get()].1 => {
                        ld.moved[*e.get()] = moved;
                    }
                    Entry::Occupied(_) => {}
                    Entry::Vacant(e) => {
                        e.insert(ld.moved.len());
                        ld.moved.push(moved);
                    }
                }
                continue;
            }
            // $ld$<action>$os<version>$<arg>, for the target's version.
            let Some((action, rest)) = mold_common::bytes::split_once(rest, b'$') else { continue };
            let Some((version, arg)) =
                rest.strip_prefix(b"os").and_then(|r| mold_common::bytes::split_once(r, b'$'))
            else {
                continue;
            };
            if arg.is_empty() || directive_version(version) != Some(minos) {
                continue;
            }
            match action {
                b"add" => ld.added.push(arg),
                b"hide" => _ = ld.hidden.insert(arg),
                b"install_name" if ld.install_name.is_none_or(|last| last < arg) => {
                    ld.install_name = Some(arg);
                }
                b"compatibility_version"
                    if ld.compatibility_version.is_none_or(|(first, _)| name < first) =>
                {
                    if let Some(version) = directive_version(arg) {
                        ld.compatibility_version = Some((name, version));
                    }
                }
                _ => {}
            }
        }
        ld
    }

    /// Whether the library keeps an export: it is no directive and not
    /// hidden.
    pub fn keeps(&self, name: &[u8]) -> bool {
        !name.starts_with(b"$ld$") && !self.hidden.contains(name)
    }

    /// The install name the library takes from a directive, if any.
    pub fn renamed_install_name(&self) -> Option<&'static [u8]> {
        self.previous.map(|(name, _)| name).or(self.install_name)
    }

    /// The version the library takes with an older one's install name,
    /// if the directive gives one.
    pub fn renamed_version(&self) -> Option<u32> {
        self.previous.and_then(|(_, version)| version)
    }

    /// The directives' effect beyond the exports, for a library at
    /// `current_version` and `compatibility_version` (after renaming).
    pub fn finish(self, current_version: u32, compatibility_version: u32) -> LdDirectives {
        let renamed = self.renamed_install_name().is_some();
        let moved = self
            .moved
            .into_iter()
            .map(|(name, install_name, version)| MovedExport {
                name,
                install_name,
                current_version: version.unwrap_or(current_version),
                compatibility_version: version.unwrap_or(compatibility_version),
            })
            .collect();
        LdDirectives { renamed, moved }
    }
}

/// Applies a .tbd's "$ld$..." export names (see LdSymbols) to it.
pub fn interpret_ld_symbols<E: crate::arch::Target>(
    ctx: &Context<E>,
    tbd: &mut TbdFile,
) -> LdDirectives {
    let directives = std::mem::take(&mut tbd.ld_symbols);
    let ld = LdSymbols::read(ctx, &directives);
    // Without a directive among the exports there is none to drop and
    // none that hides one (see LdSymbols::keeps).
    let is_directive = |n: &&[u8]| n.starts_with(b"$ld$");
    if !directives.is_empty() || tbd.weak_exports.iter().any(is_directive) {
        tbd.exports.retain(|n| ld.keeps(n));
        tbd.weak_exports.retain(|n| ld.keeps(n));
    }
    tbd.exports.extend(&ld.added);
    if let Some(name) = ld.renamed_install_name() {
        tbd.install_name = name;
    }
    if let Some((_, version)) = ld.compatibility_version {
        tbd.compatibility_version = version;
    }
    if let Some(version) = ld.renamed_version() {
        tbd.current_version = version;
        tbd.compatibility_version = version;
    }
    ld.finish(tbd.current_version, tbd.compatibility_version)
}

/// An $ld$previous directive:
/// <install name>$<compat>$<platform>$<lo>$<hi>[$[<sym>[$]]], the
/// symbol - which may itself contain '$', as Swift's do - less a final
/// '$'.
struct PreviousDirective {
    install_name: &'static [u8],
    version: Option<u32>,
    lo: u32,
    hi: u32,
    sym: &'static [u8],
}

impl PreviousDirective {
    /// Reads a directive for `platform`; None for one for another
    /// platform (half of SwiftUICore's 30,000 are for Mac Catalyst), or
    /// one with a field that doesn't parse.
    fn parse(rest: &'static [u8], platform: u32) -> Option<Self> {
        use mold_common::bytes::split_once;
        let (install_name, rest) = split_once(rest, b'$')?;
        let (compat, rest) = split_once(rest, b'$')?;
        let (for_platform, rest) = split_once(rest, b'$')?;
        let (lo, rest) = split_once(rest, b'$')?;
        let (hi, sym) = split_once(rest, b'$').unwrap_or((rest, b""));
        if install_name.is_empty()
            || std::str::from_utf8(for_platform).ok()?.parse::<u32>().ok()? != platform
        {
            return None;
        }
        Some(Self {
            install_name,
            version: if compat.is_empty() { None } else { Some(directive_version(compat)?) },
            lo: directive_version(lo)?,
            hi: directive_version(hi)?,
            sym: sym.strip_suffix(b"$").unwrap_or(sym),
        })
    }
}

/// A version in a directive, X[.Y[.Z]], packed as a Mach-O version in
/// 16, 8 and 8 bits.
fn directive_version(s: &[u8]) -> Option<u32> {
    let mut parts = s.split(|&c| c == b'.');
    let mut version = 0;
    for (shift, max) in [(16, 0xffff), (8, 0xff), (0, 0xff)] {
        let Some(part) = parts.next() else { break };
        let n: u32 = std::str::from_utf8(part).ok()?.parse().ok()?;
        if n > max {
            return None;
        }
        version |= n << shift;
    }
    parts.next().is_none().then_some(version)
}

/// Parses a TBD v1-4 file: YAML documents, the first the library itself
/// and the others the libraries it re-exports, inlined.
fn parse_yaml(
    file: &Path,
    text: &'static str,
    arch: &'static str,
    platform: u32,
) -> Option<TbdFile> {
    // The target a document is read for, with the platforms it has for
    // the architecture, if it has a target on the architecture.
    let select = |fields: &[YamlField]| {
        let top = || fields.iter().filter(|f| f.indent == 0 && !f.item);
        let (target, platforms) = select_target(arch, platform, &yaml_targets(top()));
        yaml_matches(top(), target).then_some((target, platforms))
    };
    let docs = yaml_documents(text);
    let (target, platforms) = select(&docs[0])?;
    let mut tbd = TbdFile { platforms, ..parse_yaml_document(&docs[0], target) };
    for fields in &docs[1..] {
        if let Some((target, _)) = select(fields) {
            tbd.documents.push(parse_yaml_document(fields, target));
        }
    }

    if tbd.install_name.is_empty() {
        fatal!("{}: no install-name in .tbd file", file.display());
    }
    Some(tbd)
}

/// One YAML document of a .tbd file, read for `target`: the library's
/// install name, versions, flags, symbols and the names it re-exports,
/// as the target groups that apply to `target` list them.
fn parse_yaml_document(fields: &[YamlField], target: Target) -> TbdFile {
    let mut tbd = TbdFile {
        current_version: crate::macho::encode_version(1, 0, 0),
        compatibility_version: crate::macho::encode_version(1, 0, 0),
        ..TbdFile::default()
    };
    let mut active = true;
    // The top-level key whose value the field is in: the symbols an
    // "undefineds" section lists are the library's imports, which the
    // link has no use for.
    let mut section = "";
    for (i, field) in fields.iter().enumerate() {
        if field.indent == 0 && !field.item {
            active = true;
            section = field.key;
        }
        // A list item is a target group, which applies if its targets
        // (or architectures) include `target`.
        if field.item {
            let end = fields[i + 1..]
                .iter()
                .position(|f| f.indent <= field.indent)
                .map_or(fields.len(), |n| i + 1 + n);
            active = yaml_matches(fields[i..end].iter(), target);
        }
        if field.key == "install-name" {
            tbd.install_name = unquote(field.value).as_bytes();
        }
        if !active || section == "undefineds" {
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
                tbd.parent_umbrella = Some(unquote(field.value).as_bytes());
            }
            // Version 1 spells it allowed-clients.
            "allowable-clients" | "allowed-clients" | "clients" => {
                tbd.allowable_clients.extend(field.items().map(str::as_bytes))
            }
            "symbols" => tbd.exports.extend(field.items().map(str::as_bytes)),
            "weak-symbols" | "weak-def-symbols" => {
                tbd.weak_exports.extend(field.items().map(str::as_bytes))
            }
            "thread-local-symbols" => tbd.tlv_exports.extend(field.items().map(str::as_bytes)),
            "libraries" | "re-exports" => {
                for name in field.items().map(str::as_bytes) {
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
                    tbd.exports.push(leak_name("_OBJC_IVAR_$_", item));
                }
            }
            _ => {}
        }
    }
    tbd
}

// Retain indentation and list-item boundaries so a target selector applies
// to its whole group, even when it follows the symbols it qualifies.
struct YamlField {
    indent: usize,
    item: bool,
    /// The key, unquoted.
    key: &'static str,
    /// The value: a scalar, a flow sequence ("[ a, b ]", perhaps over
    /// several lines), the lines of a block sequence of scalars ("- a"
    /// and on) or empty, for a nested mapping or a block sequence of
    /// them in the fields that follow.
    value: &'static str,
}

impl YamlField {
    /// The items of a sequence, unquoted. (A stub's export lists are
    /// most of its bytes, an item a line, indented: the items are split
    /// with memchr and their blanks skipped byte by byte.)
    fn items(&self) -> impl Iterator<Item = &'static str> {
        let block = self.value.starts_with("- ");
        let (body, sep) = match block {
            true => (self.value, b'\n'),
            false => (self.value.trim_start_matches('[').trim_end_matches(']'), b','),
        };
        let mut start = 0;
        let ends = memchr::memchr_iter(sep, body.as_bytes()).chain([body.len()]);
        let items = ends.map(move |end| {
            let item = &body[start..end];
            start = end + 1;
            if block { trim_start(item).trim_start_matches('-') } else { item }
        });
        items.map(unquote).filter(|s| !s.is_empty())
    }
}

/// Whether a byte is an ASCII character that str::trim takes for
/// white space.
fn is_blank(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// `s.trim_start()`, which tests its characters byte by byte up to
/// the first that isn't ASCII.
fn trim_start(s: &str) -> &str {
    let rest = &s[s.bytes().position(|b| !is_blank(b)).unwrap_or(s.len())..];
    if rest.as_bytes().first().is_some_and(|&b| !b.is_ascii()) { rest.trim_start() } else { rest }
}

/// `s.trim_end()`, likewise.
fn trim_end(s: &str) -> &str {
    let rest = &s[..s.bytes().rposition(|b| !is_blank(b)).map_or(0, |i| i + 1)];
    if rest.as_bytes().last().is_some_and(|&b| !b.is_ascii()) { rest.trim_end() } else { rest }
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

/// The fields of each YAML document of a .tbd.
fn yaml_documents(text: &'static str) -> Vec<Vec<YamlField>> {
    let bytes = text.as_bytes();
    let mut docs: Vec<Vec<YamlField>> = vec![Vec::new()];
    let mut pos = 0;
    while pos < bytes.len() {
        let eol = memchr_from(bytes, b'\n', pos).unwrap_or(bytes.len());
        let raw = &text[pos..eol];
        let line = raw.trim_start();
        let mut next = eol + 1;
        if line.starts_with("---") && !docs.last().unwrap().is_empty() {
            docs.push(Vec::new());
        }
        let fields = docs.last_mut().unwrap();
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
            fields.push(YamlField {
                indent: raw.len() - line.len(),
                item: line.starts_with("- "),
                key: unquote(key),
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

    /// A stub's names as text, to compare.
    fn strs<'a>(names: &[&'a [u8]]) -> Vec<&'a str> {
        names.iter().map(|name| std::str::from_utf8(name).unwrap()).collect()
    }

    fn mapped(text: &'static str) -> &'static MappedFile {
        Box::leak(Box::new(MappedFile {
            name: std::path::PathBuf::from("test.tbd"),
            data: text.as_bytes(),
            parent: None,
            mtime: None,
        }))
    }

    #[test]
    fn trims_as_str_does() {
        for s in [
            "",
            " ",
            " \n\t  'a b' \r\n",
            "\u{b}\u{c}x\u{b}",
            "\u{a0} \u{a0}x \u{2003}",
            "é ",
            " é",
        ] {
            assert_eq!(trim_start(s), s.trim_start(), "{s:?}");
            assert_eq!(trim_end(s), s.trim_end(), "{s:?}");
        }
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
        assert_eq!(strs(&arm.exports), ["_arm", "_OBJC_CLASS_$_Arm", "_OBJC_METACLASS_$_Arm"]);
        assert_eq!(strs(&arm.weak_exports), ["_weak_arm"]);
        assert_eq!(strs(&arm.tlv_exports), ["_tls_arm"]);
        assert_eq!(strs(&arm.reexports), ["/arm"]);
        let x86 = parse_cached(mf, "x86_64", PLATFORM_MACOS).unwrap();
        assert_eq!(strs(&x86.exports), ["_x86"]);
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
        assert_eq!(strs(&tbd.exports), ["_arm"]);
        assert!(tbd.reexports.is_empty());
        assert_eq!(strs(&tbd.document(b"/inline").unwrap().exports), ["_fallback"]);
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
        assert_eq!(strs(&parse(mf, "arm64", PLATFORM_MACOS).unwrap().exports), ["_arm"]);
        assert_eq!(strs(&parse(mf, "x86_64", PLATFORM_MACOS).unwrap().exports), ["_x86"]);
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
        assert_eq!(strs(&arm.exports), ["_both"]);
        assert_eq!(strs(&arm.weak_exports), ["_weak"]);
        assert_eq!(strs(&arm.tlv_exports), ["_tls"]);
        assert_eq!(strs(&arm.reexports), ["/arm"]);
        assert_eq!(strs(&arm.document(b"/inline").unwrap().exports), ["_inline"]);
        let x86 = parse(mf, "x86_64", PLATFORM_MACOS).unwrap();
        assert_eq!(strs(&x86.exports), ["_both"]);
        assert!(x86.weak_exports.is_empty());
        assert!(x86.tlv_exports.is_empty());
        assert_eq!(strs(&x86.reexports), ["/x86"]);
        assert!(x86.documents.is_empty());
    }
}
