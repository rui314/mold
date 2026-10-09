//! Resolves symbols across COFF objects and archive members, selects COMDAT
//! sections, and garbage-collects unreferenced COMDAT sections. The surviving
//! sections are laid out and written by `image`.
//!
//! Inputs are read in command-line order, as lld does. An object is included
//! when it is read. For an archive, the names its members define are
//! remembered, and a member is queued if one of its names is already
//! referenced and undefined. Queued members are included, first in first out,
//! after all inputs are read, and referencing a remembered name queues its
//! member. Including members in that breadth-first order places them in the
//! image where lld does.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::rc::Rc;

use mold_common::archive_file;
use mold_common::fatal;
use mold_common::mapped_file::MappedFile;
use mold_common::output_file::OutputFile;

use crate::arch;
use crate::args::{self, Options};
use crate::coff::{self, Object};
use crate::image;

/// The value of a chunk index that refers to nothing.
pub(crate) const NO_CHUNK: u32 = u32::MAX;

/// The size of an archive member header, which precedes the member's contents.
const AR_HEADER_SIZE: usize = 60;

/// Where a symbol of an input object resolves to.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Loc {
    /// A symbol the link doesn't need, such as a debug or file symbol.
    None,
    /// A position in a chunk, relative to the start of the chunk's section.
    Chunk { chunk: u32, value: u32 },
    /// An absolute value.
    Abs(u32),
    /// A global name, resolved through `Linker::globals`.
    Global(u32),
}

/// The definition of a global name.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Def {
    None,
    Chunk { chunk: u32, value: u32 },
    Abs(u32),
}

/// A section of an input object that the link keeps.
pub(crate) struct Chunk {
    pub obj: u32,
    /// Index into the sections of `obj`.
    pub sec: u32,
    pub comdat: bool,
    /// Set if another COMDAT section won the selection for the same name.
    pub discarded: bool,
    /// For an associative COMDAT, the chunk it belongs to, which must be kept too.
    pub owner: u32,
    pub live: bool,
    pub uninit: bool,
    pub size: u32,
    pub align: u32,
    /// The offset within its output section, which `image` sets.
    pub offset: u32,
}

/// A global name: a defined symbol, or one that some object refers to.
pub(crate) struct Global<'a> {
    pub name: &'a [u8],
    pub def: Def,
    /// For a weak external, the name to use if nothing defines this one.
    pub weak_default: Option<u32>,
    pub referenced: bool,
    /// The object that first referred to this name, or None for a root.
    pub referenced_by: Option<u32>,
}

pub(crate) struct Linker<'a> {
    pub opts: Options,
    pub objs: Vec<Rc<Object<'a>>>,
    pub included: Vec<bool>,
    /// The order in which objects were included, which lld uses to order its chunks.
    pub file_seq: Vec<u32>,
    /// The resolved locations of each object's symbols, indexed by symbol.
    pub locs: Vec<Vec<Loc>>,
    /// The chunk of each section of each object, or NO_CHUNK.
    pub sec_chunks: Vec<Vec<u32>>,
    pub chunks: Vec<Chunk>,
    pub globals: Vec<Global<'a>>,
    global_ids: HashMap<&'a [u8], u32>,
    /// The archive member that defines each remembered name.
    lazy: HashMap<&'a [u8], u32>,
    /// Archive members waiting to be included, and whether each was queued.
    queue: VecDeque<u32>,
    queued: Vec<bool>,
    /// The number of objects included so far.
    next_seq: u32,
    /// The names that garbage collection keeps: the entry point and /include: symbols.
    roots: Vec<u32>,
}

impl<'a> Linker<'a> {
    fn new(opts: Options) -> Self {
        Linker {
            opts,
            objs: Vec::new(),
            included: Vec::new(),
            file_seq: Vec::new(),
            locs: Vec::new(),
            sec_chunks: Vec::new(),
            chunks: Vec::new(),
            globals: Vec::new(),
            global_ids: HashMap::new(),
            lazy: HashMap::new(),
            queue: VecDeque::new(),
            queued: Vec::new(),
            next_seq: 0,
            roots: Vec::new(),
        }
    }

    fn push_object(&mut self, obj: Object<'a>) -> u32 {
        self.objs.push(Rc::new(obj));
        self.included.push(false);
        self.file_seq.push(u32::MAX);
        self.locs.push(Vec::new());
        self.sec_chunks.push(Vec::new());
        self.queued.push(false);
        (self.objs.len() - 1) as u32
    }

    /// Adds an input file. An object is included at once. For an archive, a
    /// member is queued when it defines a name that is referenced already.
    fn read_input(&mut self, path: &Path) {
        let mf = MappedFile::must_open(path);
        let data = mf.data();
        if data.starts_with(b"!<thin>\n") {
            fatal!("{}: thin archives are not supported", path.display());
        }
        if data.starts_with(b"!<arch>\n") {
            let mut member_at: HashMap<usize, u32> = HashMap::new();
            for member in archive_file::read_archive_members(Path::new(""), mf) {
                let mdata = member.data();
                // Members that aren't COFF objects, such as rustc's metadata
                // and import libraries, hold nothing for the image.
                if !coff::is_coff_object(mdata) {
                    continue;
                }
                let name = format!("{}({})", path.display(), member.name.display());
                let oi = self.push_object(parse_object(name, mdata));
                member_at.insert(member.offset() - AR_HEADER_SIZE, oi);
            }

            // Like lld, go through the symbol table in its order. A name that
            // is referenced and undefined queues its member. A name that is
            // already defined, or is weakly referenced, is left alone, and
            // any other name is remembered, first one first.
            for (name, offset) in archive_symbols(data) {
                let Some(&oi) = member_at.get(&offset) else { continue };
                if let Some(&gid) = self.global_ids.get(name) {
                    let g = &self.globals[gid as usize];
                    if g.def != Def::None || g.weak_default.is_some() {
                        continue;
                    }
                    if g.referenced {
                        self.fetch(oi);
                        continue;
                    }
                }
                self.lazy.entry(name).or_insert(oi);
            }
        } else {
            if !coff::is_coff_object(data) {
                fatal!("{}: unknown file type", path.display());
            }
            let oi = self.push_object(parse_object(path.display().to_string(), data));
            self.include(oi);
        }
    }

    /// Queues an archive member, once.
    fn fetch(&mut self, oi: u32) {
        if !self.included[oi as usize] && !self.queued[oi as usize] {
            self.queued[oi as usize] = true;
            self.queue.push_back(oi);
        }
    }

    /// Includes the queued archive members. Including one may queue more.
    fn include_queued(&mut self) {
        while let Some(oi) = self.queue.pop_front() {
            self.include(oi);
        }
    }

    fn intern(&mut self, name: &'a [u8]) -> u32 {
        if let Some(&gid) = self.global_ids.get(name) {
            return gid;
        }
        let gid = self.globals.len() as u32;
        self.globals.push(Global {
            name,
            def: Def::None,
            weak_default: None,
            referenced: false,
            referenced_by: None,
        });
        self.global_ids.insert(name, gid);
        gid
    }

    /// Records a strong reference to `gid`. If nothing defines it, but a
    /// remembered archive member does, that member is queued.
    fn need(&mut self, gid: u32, by: Option<u32>) {
        let g = &mut self.globals[gid as usize];
        if !g.referenced {
            g.referenced = true;
            g.referenced_by = by;
        }
        if g.def != Def::None || g.weak_default.is_some() {
            return;
        }
        if let Some(&oi) = self.lazy.get(g.name) {
            self.fetch(oi);
        }
    }

    fn define(&mut self, gid: u32, def: Def, obj: u32) {
        let existing = self.globals[gid as usize].def;
        if existing != Def::None {
            let first = match existing {
                Def::Chunk { chunk, .. } => {
                    self.objs[self.chunks[chunk as usize].obj as usize].name.clone()
                }
                _ => "an absolute symbol".to_string(),
            };
            fatal!(
                "duplicate symbol: {}\n>>> defined in {}\n>>> defined in {}",
                String::from_utf8_lossy(self.globals[gid as usize].name),
                first,
                self.objs[obj as usize].name,
            );
        }
        self.globals[gid as usize].def = def;
    }

    /// Adds the sections and symbols of object `oi` to the link. Names that
    /// it refers to are recorded, and may queue archive members.
    fn include(&mut self, oi: u32) {
        self.included[oi as usize] = true;
        let obj = Rc::clone(&self.objs[oi as usize]);

        let mut sec_chunks = vec![NO_CHUNK; obj.sections.len()];
        for (si, sec) in obj.sections.iter().enumerate() {
            if sec.name.starts_with(b".debug")
                || sec.characteristics & (coff::SCN_LNK_INFO | coff::SCN_LNK_REMOVE) != 0
            {
                continue;
            }
            sec_chunks[si] = self.chunks.len() as u32;
            self.chunks.push(Chunk {
                obj: oi,
                sec: si as u32,
                comdat: sec.comdat.is_some(),
                discarded: false,
                owner: NO_CHUNK,
                live: false,
                uninit: sec.characteristics & coff::SCN_CNT_UNINITIALIZED_DATA != 0,
                size: sec.size,
                align: align_of(sec.characteristics),
                offset: 0,
            });
        }

        for (si, sec) in obj.sections.iter().enumerate() {
            let c = sec_chunks[si];
            let Some(cd) = sec.comdat else { continue };
            if c == NO_CHUNK || cd.selection != coff::SEL_ASSOCIATIVE {
                continue;
            }
            let owner_sec = cd.associated as usize;
            if owner_sec >= 1
                && owner_sec <= sec_chunks.len()
                && sec_chunks[owner_sec - 1] != NO_CHUNK
            {
                self.chunks[c as usize].owner = sec_chunks[owner_sec - 1];
            }
        }

        // A COMDAT section loses if another one already defines one of its
        // names. This is decided before any name is defined, so that a losing
        // section never leaves some of its names defined.
        for sym in &obj.symbols {
            if sym.aux_slot || sym.storage != coff::CLASS_EXTERNAL || sym.section <= 0 {
                continue;
            }
            let c = sec_chunks.get(sym.section as usize - 1).copied().unwrap_or(NO_CHUNK);
            if c == NO_CHUNK || !self.chunks[c as usize].comdat {
                continue;
            }
            let Some(&gid) = self.global_ids.get(sym.name) else { continue };
            if let Def::Chunk { chunk, .. } = self.globals[gid as usize].def
                && self.chunks[chunk as usize].comdat
            {
                self.chunks[c as usize].discarded = true;
            }
        }

        let mut locs = vec![Loc::None; obj.symbols.len()];
        for (i, sym) in obj.symbols.iter().enumerate() {
            if sym.aux_slot {
                continue;
            }
            match sym.storage {
                coff::CLASS_FILE | coff::CLASS_SECTION => continue,
                coff::CLASS_WEAK_EXTERNAL => {
                    let gid = self.intern(sym.name);
                    locs[i] = Loc::Global(gid);
                    // A weak reference doesn't pull in a member that defines
                    // the name, and it forgets a member remembered for it.
                    // Its default is a strong reference.
                    let g = &mut self.globals[gid as usize];
                    if !g.referenced {
                        g.referenced = true;
                        g.referenced_by = Some(oi);
                    }
                    if let Some(t) = sym.weak_default {
                        let Some(target) = obj.symbols.get(t as usize) else {
                            fatal!("{}: weak external refers to symbol {t} out of range", obj.name);
                        };
                        let tg = self.intern(target.name);
                        let g = &mut self.globals[gid as usize];
                        if g.weak_default.is_none() {
                            g.weak_default = Some(tg);
                        }
                        let name = g.name;
                        if g.def == Def::None {
                            self.lazy.remove(name);
                            self.need(tg, Some(oi));
                        }
                    }
                    continue;
                }
                _ => {}
            }

            let external = sym.storage == coff::CLASS_EXTERNAL;
            if sym.section == 0 {
                if external {
                    if sym.value != 0 {
                        fatal!(
                            "{}: COMMON symbols are not supported: {}",
                            obj.name,
                            String::from_utf8_lossy(sym.name)
                        );
                    }
                    let gid = self.intern(sym.name);
                    locs[i] = Loc::Global(gid);
                    self.need(gid, Some(oi));
                }
            } else if sym.section == coff::SYM_ABSOLUTE {
                if external {
                    let gid = self.intern(sym.name);
                    self.define(gid, Def::Abs(sym.value), oi);
                    locs[i] = Loc::Global(gid);
                } else {
                    locs[i] = Loc::Abs(sym.value);
                }
            } else if sym.section > 0 {
                let Some(&c) = sec_chunks.get(sym.section as usize - 1) else {
                    fatal!("{}: invalid section number {}", obj.name, sym.section);
                };
                if c == NO_CHUNK {
                    continue;
                }
                if external {
                    let gid = self.intern(sym.name);
                    locs[i] = Loc::Global(gid);
                    if !self.chunks[c as usize].discarded {
                        self.define(gid, Def::Chunk { chunk: c, value: sym.value }, oi);
                    }
                } else {
                    locs[i] = Loc::Chunk { chunk: c, value: sym.value };
                }
            }
        }

        for sec in obj.sections.iter().filter(|s| s.name == b".drectve") {
            self.read_directives(oi, &obj.name, sec.data);
        }
        self.locs[oi as usize] = locs;
        self.sec_chunks[oi as usize] = sec_chunks;
        self.file_seq[oi as usize] = self.next_seq;
        self.next_seq += 1;
    }

    /// Reads the linker options that an object file asks for in its `.drectve`
    /// section. `/include:` names a symbol the link must keep, as lld does.
    /// Options that don't matter for an image with no imports are skipped,
    /// and others are reported.
    fn read_directives(&mut self, oi: u32, obj_name: &str, data: &'a [u8]) {
        let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
        for token in data.split(|b| b.is_ascii_whitespace() || *b == 0).filter(|t| !t.is_empty()) {
            let Some(option) = token.strip_prefix(b"/").or_else(|| token.strip_prefix(b"-")) else {
                mold_common::warn!(
                    "{obj_name}: ignoring unexpected .drectve text: {}",
                    String::from_utf8_lossy(token)
                );
                continue;
            };
            let (name, value) = match option.iter().position(|&b| b == b':') {
                Some(i) => (&option[..i], &option[i + 1..]),
                None => (option, &option[option.len()..]),
            };
            let value = value.strip_prefix(b"\"").unwrap_or(value);
            let value = value.strip_suffix(b"\"").unwrap_or(value);
            match name.to_ascii_lowercase().as_slice() {
                b"include" => {
                    let gid = self.intern(value);
                    self.roots.push(gid);
                    self.need(gid, Some(oi));
                }
                b"defaultlib" if !self.opts.no_default_lib => mold_common::warn!(
                    "{obj_name}: ignoring /defaultlib:{}, because default libraries are not supported",
                    String::from_utf8_lossy(value)
                ),
                b"defaultlib"
                | b"nodefaultlib"
                | b"failifmismatch"
                | b"manifestdependency"
                | b"release"
                | b"editandcontinue"
                | b"guardsym"
                | b"throwingnew"
                | b"inferasanlibs" => {}
                _ => mold_common::warn!(
                    "{obj_name}: ignoring unsupported .drectve option: /{}",
                    String::from_utf8_lossy(option)
                ),
            }
        }
    }

    /// Returns the location a global name resolves to, following weak external defaults.
    pub(crate) fn resolve(&self, mut gid: u32) -> Option<Loc> {
        for _ in 0..=self.globals.len() {
            let g = &self.globals[gid as usize];
            match g.def {
                Def::Chunk { chunk, value } => return Some(Loc::Chunk { chunk, value }),
                Def::Abs(v) => return Some(Loc::Abs(v)),
                Def::None => gid = g.weak_default?,
            }
        }
        None
    }

    /// Returns the chunk that a symbol location is in, if any.
    pub(crate) fn chunk_of(&self, loc: Loc) -> Option<u32> {
        let loc = match loc {
            Loc::Global(g) => self.resolve(g)?,
            other => other,
        };
        match loc {
            Loc::Chunk { chunk, .. } => Some(chunk),
            _ => None,
        }
    }

    fn check_undefined(&self) {
        let mut msgs = Vec::new();
        for g in &self.globals {
            if g.def != Def::None || g.weak_default.is_some() || !g.referenced {
                continue;
            }
            let by = match g.referenced_by {
                Some(oi) => self.objs[oi as usize].name.clone(),
                None => "a linker option or the entry point".to_string(),
            };
            msgs.push(format!(
                "undefined symbol: {}\n>>> referenced by {by}",
                String::from_utf8_lossy(g.name)
            ));
        }
        if !msgs.is_empty() {
            fatal!("{}\n{} undefined symbol(s)", msgs.join("\n"), msgs.len());
        }
    }

    /// Decides which chunks the image keeps. Without garbage collection, a
    /// chunk is kept unless it lost a COMDAT race or its owner was dropped.
    /// With it, every non-COMDAT chunk is kept, as in lld, along with the
    /// COMDAT chunks that they reach through relocations.
    fn compute_liveness(&mut self) {
        let roots = self.roots.clone();
        let n = self.chunks.len();
        if !self.opts.gc_sections {
            for c in 0..n {
                let live = self.kept_without_gc(c);
                self.chunks[c].live = live;
            }
            return;
        }

        let mut children = vec![Vec::new(); n];
        for (c, ch) in self.chunks.iter().enumerate() {
            if ch.owner != NO_CHUNK {
                children[ch.owner as usize].push(c as u32);
            }
        }
        for ch in &mut self.chunks {
            ch.live = false;
        }

        let mut stack = Vec::new();
        for c in 0..n {
            if !self.chunks[c].comdat {
                self.mark(c as u32, &children, &mut stack);
            }
        }
        for gid in roots {
            if let Some(c) = self.chunk_of(Loc::Global(gid)) {
                self.mark(c, &children, &mut stack);
            }
        }
        while let Some(c) = stack.pop() {
            let ch = &self.chunks[c as usize];
            let obj = Rc::clone(&self.objs[ch.obj as usize]);
            let targets: Vec<u32> = obj.sections[ch.sec as usize]
                .relocs
                .iter()
                .filter_map(|r| self.chunk_of(self.locs[ch.obj as usize][r.symbol as usize]))
                .collect();
            for t in targets {
                self.mark(t, &children, &mut stack);
            }
        }
    }

    fn kept_without_gc(&self, c: usize) -> bool {
        let mut cur = c as u32;
        for _ in 0..=self.chunks.len() {
            let ch = &self.chunks[cur as usize];
            if ch.discarded {
                return false;
            }
            if ch.owner == NO_CHUNK {
                return true;
            }
            cur = ch.owner;
        }
        false
    }

    fn mark(&mut self, c: u32, children: &[Vec<u32>], stack: &mut Vec<u32>) {
        let ch = &mut self.chunks[c as usize];
        if ch.live || ch.discarded {
            return;
        }
        ch.live = true;
        let owner = ch.owner;
        stack.push(c);
        if owner != NO_CHUNK {
            self.mark(owner, children, stack);
        }
        for &child in &children[c as usize] {
            self.mark(child, children, stack);
        }
    }
}

/// Returns the symbols of an archive in the order that lld goes through them,
/// each with the header offset of the member that defines it. A COFF archive
/// has two linker members, and LLVM uses the second, which is sorted by name.
/// Returns nothing if the archive has no linker member.
fn archive_symbols(data: &[u8]) -> Vec<(&[u8], usize)> {
    let mut members = Vec::new();
    let mut pos = 8;
    while members.len() < 2 {
        let Some(hdr) = data.get(pos..pos + AR_HEADER_SIZE) else { break };
        if hdr[..16].trim_ascii_end() != b"/" {
            break;
        }
        let size =
            std::str::from_utf8(&hdr[48..58]).ok().and_then(|s| s.trim().parse::<usize>().ok());
        let Some(body) =
            size.and_then(|size| data.get(pos + AR_HEADER_SIZE..pos + AR_HEADER_SIZE + size))
        else {
            break;
        };
        members.push(body);
        pos += AR_HEADER_SIZE + body.len() + (body.len() & 1);
    }
    match members[..] {
        [_, second] => second_linker_member(second),
        [first] => first_linker_member(first),
        _ => Vec::new(),
    }
}

/// Parses the first linker member: big-endian offsets, then names, in member order.
fn first_linker_member(body: &[u8]) -> Vec<(&[u8], usize)> {
    let Some(count) = body.get(..4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    else {
        return Vec::new();
    };
    let Some(offsets) = body.get(4..4 + 4 * count) else { return Vec::new() };
    let mut names = body.get(4 + 4 * count..).unwrap_or(&[]).split(|&b| b == 0);
    offsets
        .chunks_exact(4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
        .map(|offset| (names.next().unwrap_or(&[]), offset))
        .collect()
}

/// Parses the second linker member: little-endian member offsets, then for
/// each symbol the 1-based index of its member, and the symbol names sorted.
fn second_linker_member(body: &[u8]) -> Vec<(&[u8], usize)> {
    let le32 = |at: usize| {
        body.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let Some(nmembers) = le32(0) else { return Vec::new() };
    let Some(nsyms) = le32(4 + 4 * nmembers) else { return Vec::new() };
    let indices_at = 8 + 4 * nmembers;
    let Some(indices) = body.get(indices_at..indices_at + 2 * nsyms) else { return Vec::new() };
    let mut names = body.get(indices_at + 2 * nsyms..).unwrap_or(&[]).split(|&b| b == 0);
    let mut out = Vec::with_capacity(nsyms);
    for b in indices.chunks_exact(2) {
        let name = names.next().unwrap_or(&[]);
        let index = u16::from_le_bytes([b[0], b[1]]) as usize;
        if let Some(offset) = index.checked_sub(1).and_then(|i| le32(4 + 4 * i)) {
            out.push((name, offset));
        }
    }
    out
}

/// Returns the alignment in bytes that a section's characteristics request.
fn align_of(characteristics: u32) -> u32 {
    match (characteristics & coff::SCN_ALIGN_MASK) >> 20 {
        0 => 16,
        code => 1 << (code - 1),
    }
}

fn parse_object(name: String, data: &[u8]) -> Object<'_> {
    match coff::parse(name, data) {
        Ok(obj) => obj,
        Err(e) => fatal!("{e}"),
    }
}

fn default_entry(subsystem: u16) -> &'static str {
    match subsystem {
        10..=13 => "efi_main",
        args::SUBSYSTEM_WINDOWS => "WinMainCRTStartup",
        _ => "mainCRTStartup",
    }
}

/// Links the inputs named in `opts` and writes the image.
pub fn link(opts: Options) {
    let inputs = opts.inputs.clone();
    let entry_name =
        opts.entry.clone().unwrap_or_else(|| default_entry(opts.subsystem).to_string());
    let includes = opts.includes.clone();
    let output = opts.output.clone();

    let mut ln = Linker::new(opts);
    let entry = ln.intern(entry_name.as_bytes());
    ln.need(entry, None);
    ln.roots.push(entry);
    for inc in &includes {
        let gid = ln.intern(inc.as_bytes());
        ln.need(gid, None);
        ln.roots.push(gid);
    }

    for path in &inputs {
        ln.read_input(path);
    }
    ln.include_queued();
    ln.check_undefined();
    ln.compute_liveness();

    let image = image::build::<arch::x86_64::X86_64>(&mut ln, entry);

    let mut out = OutputFile::open(&output, image.len() as u64, 0o755, false, false);
    out.buf().copy_from_slice(&image);
    out.close();
}
