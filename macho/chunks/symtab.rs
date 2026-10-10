//! This file creates the symbol table, which LC_SYMTAB points to and
//! LC_DYSYMTAB divides into parts, and writes it with its string table.
//!
//! A Mach-O image has one symbol table, in __LINKEDIT, which serves the
//! purpose of ELF's .symtab: debuggers, profilers and nm read it. (dyld
//! finds the image's exports in the export trie instead; see
//! export_trie.rs.) LC_DYSYMTAB divides the table into three runs: the
//! local symbols, the symbols the image defines and exports ("external"
//! symbols, sorted by name), and the symbols it imports (sorted by name
//! too).
//!
//! The table also holds debug notes, called "stabs". A Mach-O image doesn't
//! carry DWARF debug information: the linker leaves it in the object files,
//! and for each object file with debug information it writes symbol table
//! entries that tell the debugger where the object file is (N_OSO) and
//! where each of its functions and variables ended up (N_FUN and others).
//! The debugger then reads the DWARF from the object files. The notes come
//! after the local symbols, before the external ones.
//!
//! The table is laid out before addresses are known; its entries get their
//! values as it is written (see copy_symtab), and their names are written
//! into the string table (strtab.rs) at the same time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use mold_common::mem::leak_bytes;
use mold_common::path::path_bytes;
use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::{ChunkHeader, ChunkId, OutputSectionId};
use crate::context::Context;
use crate::input_files::{
    FileId, LocalSymbol, ObjectFile, SymtabBlock, should_write_to_local_symtab,
};
use crate::macho::*;
use crate::symbol::{Symbol, SymbolId};

/// The symbol table, laid out before addresses are known. The symbol
/// slot of each entry supplies its final `value` when the table is
/// copied to the output.
#[derive(Debug)]
pub struct SymtabSection {
    pub hdr: ChunkHeader,
    /// The entries but the debug notes: those before the notes - the
    /// plain locals and N_AST paths - then the externals and imports,
    /// which follow the notes in the table.
    pub entries: Vec<(MachSym, Option<SymbolId>)>,
    /// The string table's total size (bytes, padded to 8). The bytes
    /// themselves are not materialized here: copy_symtab writes each
    /// entry's name at its stroff straight into the output.
    pub strtab_size: usize,
    /// Each entry's name, empty for one that has no string of its own
    /// (it names nothing, or shares another entry's).
    pub names: Vec<&'static [u8]>,
    /// The debug notes, one plan per object, which copy_symtab writes
    /// straight into the output as mold-rust's populate_symtab writes a
    /// file's symbols: entries [stabs_start, stabs_start + nstabs) of the
    /// table, each object's run after the previous one's, and each run's
    /// strings from its offset in `stab_strx`, whose last element is
    /// where the notes' strings end.
    pub stabs: Vec<StabPlan>,
    pub stab_strx: Vec<u32>,
    pub stabs_start: usize,
    pub nstabs: usize,
    /// Each symbol's string offset, for the notes naming it, or u32::MAX
    /// if they need a copy of their own.
    pub strx_of: Vec<u32>,
    pub nlocal: u32,
    pub nextdef: u32,
    pub nundef: u32,
    /// Each symbol's index in the output symbol table - its local's,
    /// external's or import's entry, never a debug note's - or u32::MAX
    /// if it has none, for the indirect symbol table. mold keeps output
    /// symtab indices as direct per-symbol data too, not in a map;
    /// one flat array serves here because Mach-O name-sorts its
    /// globals across all files, which rules out per-file bases.
    pub output_sym_indices: Vec<u32>,
}

impl SymtabSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::linkedit();
        hdr.p2align = 3;
        Self {
            hdr,
            entries: Vec::new(),
            strtab_size: 0,
            names: Vec::new(),
            stabs: Vec::new(),
            stab_strx: Vec::new(),
            stabs_start: 0,
            nstabs: 0,
            strx_of: Vec::new(),
            nlocal: 0,
            nextdef: 0,
            nundef: 0,
            output_sym_indices: Vec::new(),
        }
    }

    /// The number of entries in the table, the debug notes included.
    pub fn len(&self) -> usize {
        self.entries.len() + self.nstabs
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Takes the debug notes, entries [stabs_start, stabs_start + n) of
    /// the table, and lays out their strings after the entries', which
    /// end at `strtab_end`: each object's after the previous one's, but
    /// for those the notes share with the symbols they name (`strx_of`,
    /// set by now).
    pub fn set_stabs<E: Target>(
        &mut self,
        ctx: &Context<E>,
        plans: Vec<StabPlan>,
        stabs_start: usize,
        strtab_end: usize,
    ) {
        let sizes: Vec<usize> =
            plans.par_iter().map(|plan| plan.strtab_size(ctx, &self.strx_of)).collect();
        let mut strx = strtab_end;
        self.stab_strx = Vec::with_capacity(plans.len() + 1);
        for size in sizes {
            self.stab_strx.push(strx as u32);
            strx += size;
        }
        self.stab_strx.push(strx as u32);
        self.strtab_size = strx.next_multiple_of(8);
        self.nstabs = plans.iter().map(StabPlan::len).sum();
        self.stabs = plans;
        self.stabs_start = stabs_start;
    }
}

impl Default for SymtabSection {
    fn default() -> Self {
        Self::new()
    }
}

/// The -add_ast_path paths the symbol table lists after the local
/// symbols, as N_AST entries (Swift modules for the debugger): none
/// under -S, which drops the debugger's notes, nor in a -r output under
/// -x, which has none (see relocatable::build_symtab).
pub fn ast_paths<E: Target>(ctx: &Context<E>) -> &[PathBuf] {
    if ctx.args.strip_debug || (ctx.args.relocatable && ctx.args.strip_locals) {
        &[]
    } else {
        &ctx.args.add_ast_paths
    }
}

/// Adds the N_AST entries of ast_paths, final image or -r output.
pub fn push_ast_paths<E: Target>(
    ctx: &Context<E>,
    names: &mut Vec<&'static [u8]>,
    entries: &mut Vec<(MachSym, Option<SymbolId>)>,
) {
    for path in ast_paths(ctx) {
        names.push(leak_bytes(path_bytes(path).to_vec()));
        entries.push((MachSym { stroff: U32::new(0), n_type: N_AST, ..Default::default() }, None));
    }
}

/// A non-external symbol's name as ld-prime writes it in the output's
/// symbol table: cut at its first ".llvm.", which ThinLTO appends with
/// a hash of the module to the statics it promotes to global scope, so
/// that the debugger sees the name the source gave it. (The map keeps
/// the whole name.)
pub fn local_symbol_name(name: &[u8]) -> &[u8] {
    // A Finder made once, not one for each name.
    static LLVM: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(".llvm."));
    LLVM.find(name).map_or(name, |i| &name[..i])
}

/// One stab entry: its name and MachSym, the symbol whose final address
/// fills in `value`, and the symbol the name is, if any, whose string
/// the entry shares.
#[derive(Clone, Copy, Debug)]
pub struct Stab {
    pub name: &'static [u8],
    pub ent: MachSym,
    pub value_of: Option<SymbolId>,
    pub name_of: Option<SymbolId>,
}

impl Stab {
    fn new(name: &'static [u8], ent: MachSym, value_of: Option<SymbolId>) -> Self {
        Self { name, ent, value_of, name_of: None }
    }

    /// The string of the symbol the entry names, if it shares it.
    fn shared_strx(&self, strx_of: &[u32]) -> Option<u32> {
        let strx = strx_of[self.name_of? as usize];
        (strx != u32::MAX).then_some(strx)
    }
}

/// An object's planned stab entries: those written as they are (the
/// N_SO pair and N_OSO that open a run of its own, or every note a -r
/// input carries), then its symbols' notes and the N_SO closing its
/// run. A symbol's notes are kept as the symbol until written: they
/// are most of a large -g link's symbol table.
#[derive(Debug, Default)]
pub struct StabPlan {
    fixed: Vec<Stab>,
    syms: Vec<SymbolStabs>,
    closed: bool,
    len: usize,
}

impl StabPlan {
    pub fn len(&self) -> usize {
        self.len
    }

    /// The planned entries, in order.
    pub fn stabs<'a, E: Target>(&'a self, ctx: &'a Context<E>) -> impl Iterator<Item = Stab> + 'a {
        let close = self.closed.then_some(Stab::new(b"", STAB_END, None));
        self.fixed.iter().copied().chain(self.syms.iter().flat_map(|s| s.stabs(ctx))).chain(close)
    }

    /// The bytes of string table the entries' own names take: each named
    /// entry's but those that share the string of the symbol they name
    /// (`strx_of`) - a symbol's notes name it once.
    pub fn strtab_size<E: Target>(&self, ctx: &Context<E>, strx_of: &[u32]) -> usize {
        let fixed =
            self.fixed.iter().filter(|s| !s.name.is_empty() && s.shared_strx(strx_of).is_none());
        let syms = self.syms.iter().filter(|s| strx_of[s.sym as usize] == u32::MAX);
        let size = |name: &[u8]| if name.is_empty() { 0 } else { name.len() + 1 };
        fixed.map(|s| size(s.name)).sum::<usize>() + syms.map(|s| size(s.name(ctx))).sum::<usize>()
    }

    /// Writes the entries, with their final addresses and their own names,
    /// into the object's block of the symbol table - mold-rust's
    /// populate_symtab.
    pub fn populate_symtab<E: Target>(
        &self,
        ctx: &Context<E>,
        strx_of: &[u32],
        block: &mut SymtabBlock<'_>,
    ) {
        // A function's notes take its address three times in a row.
        let mut addr = (u32::MAX, 0);
        for stab in self.stabs(ctx) {
            let mut ent = stab.ent;
            if let Some(id) = stab.value_of {
                if addr.0 != id {
                    addr = (id, ctx.symbols[id].addr(ctx));
                }
                ent.value.set(addr.1);
            }
            // A nameless entry gets the empty string after the string
            // table's leading space.
            ent.stroff.set(match stab.shared_strx(strx_of) {
                Some(strx) => strx,
                None if stab.name.is_empty() => 1,
                None => block.add_string(stab.name),
            });
            block.push(ent);
        }
    }
}

/// A symbol's debug notes: N_BNSYM, the N_FUN pair and N_ENSYM for a
/// function (`size` bytes long), an N_GSYM for global data, an N_STSYM
/// for a local's.
#[derive(Clone, Copy, Debug)]
struct SymbolStabs {
    sym: SymbolId,
    size: u32,
    sect: u8,
    n_type: u8,
}

impl SymbolStabs {
    fn len(&self) -> usize {
        if self.n_type == N_FUN { 4 } else { 1 }
    }

    /// The name the notes give the symbol: a non-external one's as its
    /// symbol table entry has it (see local_symbol_name).
    fn name<E: Target>(&self, ctx: &Context<E>) -> &'static [u8] {
        let sym = &ctx.symbols[self.sym];
        let local = !sym.is_extern() || sym.is_private_extern();
        if local { local_symbol_name(sym.name()) } else { sym.name() }
    }

    fn stabs<E: Target>(&self, ctx: &Context<E>) -> impl Iterator<Item = Stab> {
        let id = Some(self.sym);
        let name = self.name(ctx);
        let sect = self.sect;
        // Named entries get their string offsets later; the rest keep 1,
        // the empty string.
        let stab =
            |n_type, sect| MachSym { stroff: U32::new(1), n_type, sect, ..Default::default() };
        let mut out = [Stab::new(b"", stab(N_BNSYM, sect), id); 4];
        match self.n_type {
            N_FUN => {
                // ld64's shape: N_BNSYM, the N_FUN pair (the function's
                // address, then its size), N_ENSYM. Its stab reader takes
                // an N_FUN without the bracketing symbols badly (a crash
                // on a -r output that had only the pair).
                let fun = MachSym { stroff: U32::new(0), ..stab(N_FUN, sect) };
                out[1] = Stab { name_of: id, ..Stab::new(name, fun, id) };
                out[2] = Stab::new(
                    b"",
                    MachSym { value: U64::new(self.size as u64), ..stab(N_FUN, 0) },
                    None,
                );
                out[3] = Stab::new(b"", stab(N_ENSYM, sect), id);
            }
            // An N_GSYM names the global only, with no section or
            // address - the debugger looks the address up by name.
            N_GSYM => {
                let ent = MachSym { n_type: N_GSYM, ..Default::default() };
                out[0] = Stab { name, ent, value_of: None, name_of: id };
            }
            _ => {
                let ent = MachSym { stroff: U32::new(0), ..stab(N_STSYM, sect) };
                out[0] = Stab { name_of: id, ..Stab::new(name, ent, id) };
            }
        }
        out.into_iter().take(self.len())
    }
}

/// Plans every object's debug notes (stabs), on all cores: each
/// object's run is independent. Mach-O binaries don't carry DWARF;
/// instead, for each object with debug info the symbol table gets stab
/// entries telling the debugger where the object file is (N_OSO) and
/// where its functions and globals ended up, and the debugger reads the
/// DWARF from the objects. Shared by the final link and -r.
pub fn plan_stabs<E: Target>(ctx: &Context<E>) -> Vec<StabPlan> {
    if ctx.args.strip_debug {
        return Vec::new();
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    (0..ctx.objs.len())
        .into_par_iter()
        .map(|obj_idx| plan_object_stabs(ctx, obj_idx, &cwd))
        .collect()
}

/// Plans one object's debug-note stabs. An object with DWARF gets the
/// run ld64 writes: N_SO, N_OSO naming the object, N_FUN pairs and
/// N_GSYM/N_STSYM for its symbols, and a closing N_SO. An object that
/// already carries such a run (a -r output: ld64 does not merge DWARF,
/// it writes these notes) has it copied through (see
/// copy_object_stabs).
fn plan_object_stabs<E: Target>(ctx: &Context<E>, obj_idx: usize, cwd: &Path) -> StabPlan {
    let obj = &ctx.objs[obj_idx];
    if !obj.is_reachable {
        return StabPlan::default();
    }
    if obj.mach_syms.iter().any(|n| n.n_type == N_OSO) {
        return copy_object_stabs(ctx, obj_idx);
    }
    if !obj.has_debug_info {
        return StabPlan::default();
    }

    let mut plan = StabPlan { fixed: object_stabs_opening(ctx, obj, cwd), ..Default::default() };
    // The symbols' notes, in symbol-table order. ld-prime lists a
    // unit's notes by address instead, but no reader depends on that:
    // dsymutil and lldb map each unit's notes by name, and an N_FUN pair
    // stays together either way.
    for (msym, &sym_id) in obj.mach_syms.iter().zip(&obj.symbols) {
        let sym = &ctx.symbols[sym_id];
        // A tentative definition gets its note in each object that
        // declares it. A global with an assembler-local name (Swift's
        // l_OBJC_PROTOCOL_SYMREF_$_*, weak private externals) gets none,
        // as no local of that name does.
        let common = msym.is_common() && is_still_common(ctx, sym_id);
        if msym.is_stab()
            || (!common && !matches!(sym.file(), Some(FileId::Obj(o)) if o as usize == obj_idx))
            || !should_write_to_local_symtab(sym.name())
        {
            continue;
        }
        plan.syms.extend(symbol_stabs(ctx, obj, sym_id, msym, common));
    }
    plan.closed = true;
    plan.len = plan.fixed.len() + plan.syms.iter().map(|s| s.len()).sum::<usize>() + 1;
    plan
}

/// The stabs of an object that carries its own (an earlier -r output's),
/// copied through: the address-bearing entries rebased to their
/// subsections' output addresses, and those of dead subsections or ones
/// coalesced away dropped. An N_GSYM names its symbol instead, with no
/// address, and goes as the symbol does (see copy_global_stab). Each
/// unit's N_SO and N_OSO entries go through even if none of its notes
/// does, and dsymutil then maps nothing from that object.
fn copy_object_stabs<E: Target>(ctx: &Context<E>, obj_idx: usize) -> StabPlan {
    let obj = &ctx.objs[obj_idx];
    let mut out = Vec::new();
    // Entries whose value is an address in the object (sect
    // says which section); an N_FUN with an empty name holds the
    // function's size instead.
    let addressed = |n: &MachSym| {
        n.sect != 0
            && matches!(
                n.n_type,
                N_FUN | N_BNSYM | N_ENSYM | N_STSYM | N_LCSYM | N_SLINE | N_ECOMM | N_ECOML
            )
    };
    // The object's own local symbols by name, for the notes that
    // name them.
    let r = obj.local_range();
    let locals: hashbrown::HashMap<&[u8], (SymbolId, &MachSym)> = obj.mach_syms[r.clone()]
        .iter()
        .zip(&obj.symbols[r])
        .filter(|(n, _)| !n.is_stab())
        .map(|(n, &id)| (ctx.symbols[id].name(), (id, n)))
        .collect();
    let mut skip_size = false;
    let mut in_unit = false;
    for (msym, &sym_id) in obj.mach_syms.iter().zip(&obj.symbols) {
        if !msym.is_stab() {
            continue;
        }
        let mut ent = *msym;
        let name = ctx.symbols[sym_id].name();
        // A closing N_SO before the first unit (ld-prime's -r outputs
        // open their stabs with one) closes nothing: it is not copied.
        if msym.n_type == N_SO && name.is_empty() {
            if !std::mem::replace(&mut in_unit, false) {
                continue;
            }
        } else {
            in_unit = true;
        }
        // The string table starts " \0": offset 1 is the empty
        // name (a closing N_SO, an N_FUN size entry); offset 0
        // would read as the name " ", and lldb then never sees
        // the unit's end.
        ent.stroff.set(if name.is_empty() { 1 } else { 0 });
        // -reproducible (or ZERO_AR_DATE) zeroes the modification time
        // the earlier link wrote, as it does an object's own.
        if msym.n_type == N_OSO && ctx.args.zero_ar_date {
            ent.value.set(0);
        }
        if msym.n_type == N_GSYM {
            out.extend(copy_global_stab(ctx, obj_idx, name, ent, &locals));
            continue;
        }
        if addressed(msym) {
            let Some((isec, off)) = noted_subsec(ctx, obj, msym.sect, msym.value.get()) else {
                // Dead code: drop the note, and a function's size
                // entry with it.
                skip_size = msym.n_type == N_FUN;
                continue;
            };
            let isec = &ctx.isecs[isec];
            ent.value.set(isec.addr(ctx) + off);
            ent.sect = isec.sect_idx(ctx);
        } else if msym.n_type == N_FUN && skip_size {
            skip_size = false;
            continue;
        }
        let name_of = match msym.n_type {
            N_FUN | N_STSYM | N_LCSYM if !name.is_empty() => {
                locals.get(name).map(|&(id, _)| id).or_else(|| ctx.symbols.lookup(name))
            }
            _ => None,
        };
        out.push(Stab { name, ent, value_of: None, name_of });
    }
    StabPlan { len: out.len(), fixed: out, ..Default::default() }
}

/// An N_GSYM copied from an earlier -r output, which ld-prime takes by
/// the symbol it names: kept, with no address, if the object still
/// defines the symbol, or it is still a tentative definition, which the
/// -r link passed on, and dropped if another file's definition won.
/// A symbol that is one of the object's locals - a private external
/// the -r link demoted - gets an N_STSYM of its address instead, as it
/// would have had in a unit with DWARF.
fn copy_global_stab<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
    name: &'static [u8],
    ent: MachSym,
    locals: &hashbrown::HashMap<&[u8], (SymbolId, &MachSym)>,
) -> Option<Stab> {
    let obj = &ctx.objs[obj_idx];
    if let Some(&(id, msym)) = locals.get(name) {
        let sect = if msym.ty() == N_ABS {
            0
        } else {
            let (isec, _) = noted_subsec(ctx, obj, msym.sect, msym.value.get())?;
            ctx.isecs[isec].sect_idx(ctx)
        };
        let ent = MachSym { n_type: N_STSYM, sect, ..ent };
        return Some(Stab { name, ent, value_of: Some(id), name_of: Some(id) });
    }
    let id = ctx.symbols.lookup(name)?;
    let own = matches!(ctx.symbols[id].file(), Some(FileId::Obj(o)) if o as usize == obj_idx);
    (own || is_still_common(ctx, id)).then(|| {
        let ent = MachSym { sect: 0, value: U64::new(0), ..ent };
        Stab { name, ent, value_of: None, name_of: Some(id) }
    })
}

/// The live subsection holding an object's symbol or note at
/// `sect`/`value`, with the offset in it, unless it was coalesced
/// away (see is_coalesced_away): ld-prime notes the survivor's
/// symbols only.
fn noted_subsec<E: Target>(
    ctx: &Context<E>,
    obj: &ObjectFile,
    sect: u8,
    value: u64,
) -> Option<(usize, u64)> {
    let (isec, off) = obj.find_symbol_subsec(&ctx.isecs, sect, value)?;
    if is_coalesced_away(ctx, isec) {
        return None;
    }
    let isec = ctx.isecs.resolve(isec);
    ctx.isecs[isec].is_alive().then_some((isec, off))
}

/// Whether a subsection gave way to another input's copy: a weak
/// definition another file's won, or a function folded into an
/// identical one. One the linker rewrote into a record of its own
/// (a class's ro data after category merging) is still there.
pub(crate) fn is_coalesced_away<E: Target>(ctx: &Context<E>, isec: usize) -> bool {
    let replacement = ctx.isecs[isec].replacement;
    replacement != crate::input_sections::NO_REPLACEMENT
        && !ctx.is_internal(ctx.isecs[replacement as usize].file as usize)
}

/// The entries that open an object's run of notes. ld64 opens each
/// object's run with two N_SO entries, the directory of the source file
/// (with a trailing slash) and its leaf name, split at the last slash
/// of its path in the DWARF compile unit: the unit's name, under its
/// compilation directory unless absolute, joined as they are ("/" and
/// "f.c" make "//" and "f.c"). Its own stab reader takes an N_SO with
/// an empty name as the closing one, so a -r output without them
/// crashed it. N_OSO then points at the object (or "archive(member)"),
/// as an absolute path; a fat file's slice goes by the file's own.
pub(crate) fn object_stabs_opening<E: Target>(
    ctx: &Context<E>,
    obj: &ObjectFile,
    cwd: &Path,
) -> Vec<Stab> {
    let mut out = Vec::new();
    let (dir, name) = match crate::dwarf::compile_unit_name(obj.mf.data(), &obj.sect_hdrs) {
        Some((dir, name)) => (dir, name),
        None => {
            let leaf = obj.mf.name.file_name().map_or(&[][..], |f| f.as_encoded_bytes());
            (Vec::new(), leaf.to_vec())
        }
    };
    let path = if name.starts_with(b"/") {
        name
    } else {
        let dir = if dir.is_empty() { path_bytes(cwd) } else { &dir };
        [dir, b"/", &name].concat()
    };
    let (dir, file) = path.split_at(path.iter().rposition(|&c| c == b'/').map_or(0, |i| i + 1));
    for name in [dir, file] {
        let name = leak_bytes(name.to_vec());
        out.push(Stab::new(name, MachSym { n_type: N_SO, ..Default::default() }, None));
    }
    let path = match obj.mf.parent {
        Some(parent) if parent.name.is_absolute() => obj.mf.name.clone(),
        Some(_) | None if obj.mf.name.is_absolute() => obj.mf.name.clone(),
        // An object ThinLTO compiled in memory has no name, here either.
        None if obj.mf.name.as_os_str().is_empty() => std::path::PathBuf::new(),
        _ => cwd.join(&obj.mf.name),
    };
    let path = crate::filetype::without_fat_arch(path_bytes(&path));
    let mut oso_name = path.clone();
    // -oso_prefix strips a leading path from every N_OSO, so
    // debug builds relocated to another machine (or built in a
    // sandbox) can still find their objects relative to a
    // debugger's source map. "." means the current directory.
    if let Some(prefix) = &ctx.args.oso_prefix {
        let mut cwd_prefix = path_bytes(cwd).to_vec();
        cwd_prefix.push(b'/');
        let prefix: &[u8] = if prefix == b"." { &cwd_prefix } else { prefix };
        if let Some(rest) = oso_name.strip_prefix(prefix) {
            oso_name = rest.to_vec();
        }
    }
    // The value is the object's modification time, which dsymutil and
    // lldb compare against the file they find (0 disables the check):
    // an archive member's is its header's, and the LTO object's 0
    // unless -object_path_lto wrote it out. ZERO_AR_DATE, set to
    // anything, or -reproducible zeroes them all.
    let mtime = if ctx.args.zero_ar_date {
        0
    } else if let Some(date) = obj.mf.mtime {
        date
    } else {
        std::fs::metadata(mold_common::bytes::os_str(&path))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs())
    };
    out.push(Stab::new(
        leak_bytes(oso_name),
        MachSym {
            stroff: U32::new(0),
            n_type: N_OSO,
            sect: E::CPUSUBTYPE as u8,
            desc: U16::new(1),
            value: U64::new(mtime),
        },
        None,
    ));
    out
}

/// A symbol's debug notes, if it gets any: every symbol of a live
/// subsection gets them, but a symbol without a section has no address
/// to note. A -r output keeps a common undefined; it is noted by name.
/// (ld-prime notes an absolute symbol too, and no symbol of the sections
/// it splits by content.)
fn symbol_stabs<E: Target>(
    ctx: &Context<E>,
    obj: &ObjectFile,
    sym_id: SymbolId,
    msym: &MachSym,
    common: bool,
) -> Option<SymbolStabs> {
    let sym = &ctx.symbols[sym_id];
    let global = SymbolStabs { sym: sym_id, size: 0, sect: 0, n_type: N_GSYM };
    let Some(isec) = sym.input_section().map(|i| i as usize) else {
        return common.then_some(global);
    };
    // The symbol has moved to the survivor if its subsection was
    // coalesced away; its own is the one to look at.
    if msym.ty() == N_SECT && noted_subsec(ctx, obj, msym.sect, msym.value.get()).is_none() {
        return None;
    }
    let isec = &ctx.isecs[ctx.isecs.resolve(isec)];
    let hdr = isec.hdr(&ctx.objs[isec.file as usize]);
    if !isec.is_alive() {
        return None;
    }
    let sect = isec.sect_idx(ctx);
    let is_text = hdr.segname_is(b"__TEXT")
        && hdr.flags.get() & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0;
    Some(if is_text {
        SymbolStabs { size: isec.size, sect, n_type: N_FUN, ..global }
    } else if msym.is_extern() {
        global
    } else {
        SymbolStabs { sect, n_type: N_STSYM, ..global }
    })
}

/// Whether a symbol is still a tentative definition, which no real one
/// replaced: a -r output passes it on as one, and a final image gives it
/// storage of the linker's own.
fn is_still_common<E: Target>(ctx: &Context<E>, id: SymbolId) -> bool {
    let sym = &ctx.symbols[id];
    sym.is_common() || matches!(sym.file(), Some(FileId::Obj(o)) if ctx.is_internal(o as usize))
}

/// An N_SO with an empty name: it closes an object's stabs.
pub const STAB_END: MachSym =
    MachSym { stroff: U32::new(1), n_type: N_SO, sect: 1, desc: U16::new(0), value: U64::new(0) };

/// A final image's local symbols: each object's non-external symbols
/// it keeps, in its symbol table's order (see
/// ObjectFile::populate_symtab), then the private externals
/// demoted to locals, then the linker's own names and the objc_msgSend$
/// stubs. -x drops them all, the demoted private externals too, as ld64
/// lists no local symbol under it. (ld-prime lists them by address, the
/// names at one address by rank, a subsection's own name last.)
fn plan_local_symbols<E: Target>(ctx: &Context<E>, pexts: &[usize]) -> Vec<NamedEntry> {
    if ctx.args.strip_locals {
        return Vec::new();
    }
    let per_obj: Vec<Vec<LocalSymbol>> =
        ctx.objs.par_iter().enumerate().map(|(i, obj)| obj.populate_symtab(ctx, i)).collect();
    let mut ents: Vec<NamedEntry> = Vec::with_capacity(per_obj.iter().map(Vec::len).sum());
    ents.extend(per_obj.into_iter().flatten().map(|l| (l.name, l.msym, l.sym)));

    // Private external symbols resolve globally but appear as locals
    // (with N_PEXT still set) in the output.
    for &i in pexts {
        let sym = &ctx.symbols[i];
        let id = Some(i as SymbolId);
        let (ent, id) = match (sym.file(), sym.input_section()) {
            (_, Some(isec)) => {
                let isec = &ctx.isecs[ctx.isecs.resolve(isec as usize)];
                (MachSym { n_type: N_SECT | N_PEXT, ..local_msym(isec.sect_idx(ctx), 0) }, id)
            }
            // A hidden __mh_execute_header (an export list that omits
            // it, or -no_exported_symbols) sits in the first section,
            // the mach header.
            (Some(FileId::Obj(o)), None) if ctx.is_internal(o as usize) => {
                (MachSym { n_type: N_SECT | N_PEXT, ..local_msym(1, 0) }, id)
            }
            (_, None) => (MachSym { n_type: N_ABS | N_PEXT, ..local_msym(0, sym.value) }, None),
        };
        // A demoted weak definition keeps N_WEAK_DEF.
        let desc = if sym.is_weak_def() { N_WEAK_DEF } else { 0 };
        ents.push((local_symbol_name(sym.name()), MachSym { desc: U16::new(desc), ..ent }, id));
    }
    ents.extend(linker_locals(ctx));
    ents
}

/// The local names the linker gives its own code and data: on
/// synthesized data, then those each chunk of the linker's own lists
/// (see chunks::populate_symtab): the objc_msgSend$ stubs, the
/// lazy-load helpers and slots, the delay-init stubs and helpers, and
/// the range-extension thunks' entries.
fn linker_locals<E: Target>(ctx: &Context<E>) -> Vec<NamedEntry> {
    let mut ents = Vec::new();
    // Locals the linker named itself, on synthesized data whose
    // addresses are final by now.
    for &(name, isec) in &ctx.extra_local_syms {
        let sec = &ctx.isecs[isec as usize];
        if sec.is_alive() && sec.output_section().is_some() {
            let ent = local_msym(sec.sect_idx(ctx), sec.addr(ctx));
            ents.push((name, ent, None));
        }
    }
    let chunks = [
        ChunkId::ObjcStubs,
        ChunkId::LazyHelpers,
        ChunkId::LazyLoadGot,
        ChunkId::DelayStubs,
        ChunkId::DelayHelper,
    ];
    let osecs =
        (0..ctx.output_sections.len() as u32).map(|i| ChunkId::Output(OutputSectionId::new(i)));
    for id in chunks.into_iter().chain(osecs) {
        crate::chunks::populate_symtab(ctx, id, &mut ents);
    }
    ents
}

/// A local symbol's entry, in section `sect`.
pub fn local_msym(sect: u8, value: u64) -> MachSym {
    MachSym { stroff: U32::new(0), n_type: N_SECT, sect, desc: U16::new(0), value: U64::new(value) }
}

/// A symbol table entry with its name, and the symbol whose address
/// fills `value`.
pub type NamedEntry = (&'static [u8], MachSym, Option<SymbolId>);

/// Appends an entry and its name for each item, made by `f` on all cores
/// straight into the arrays' spare capacity, which the caller reserved.
pub fn par_push_entries<T: Sync>(
    names: &mut Vec<&'static [u8]>,
    entries: &mut Vec<(MachSym, Option<SymbolId>)>,
    items: &[T],
    f: impl Fn(&T) -> NamedEntry + Sync,
) {
    let n = items.len();
    names.spare_capacity_mut()[..n]
        .par_iter_mut()
        .zip(&mut entries.spare_capacity_mut()[..n])
        .zip(items)
        .for_each(|((name, ent), item)| {
            let (n, e, sym) = f(item);
            name.write(n);
            ent.write((e, sym));
        });
    // SAFETY: the n slots past each array's end were written above.
    unsafe {
        names.set_len(names.len() + n);
        entries.set_len(entries.len() + n);
    }
}

/// A sort key that orders byte strings like the strings themselves but
/// settles most comparisons on one integer: the first eight bytes,
/// big-endian, zero-padded. Symbol names cannot contain NULs, so
/// (prefix, name) order equals plain name order. Mach-O sorts its
/// global symbols and export-trie input by name (ELF mold never
/// name-sorts), and mangled names share long prefixes, which makes
/// plain slice comparison the sort's bottleneck.
pub fn name_sort_key(name: &[u8]) -> (u64, &[u8]) {
    let mut p = [0u8; 8];
    let n = name.len().min(8);
    p[..n].copy_from_slice(&name[..n]);
    (u64::from_be_bytes(p), name)
}

/// Builds the output symbol table contents: the local symbols (see
/// plan_local_symbols), N_AST paths and debug notes, then the defined
/// globals and the imports, each sorted by name. Symbol values are
/// filled in when the table is copied out, after addresses are
/// assigned.
pub fn create_output_symtab<E: Target>(
    ctx: &Context<E>,
    sorted_globals: &[SymbolId],
) -> SymtabSection {
    let mut data = SymtabSection::new();

    let t = ctx.timer("symtab-classify");
    let indexed = indexed_imports(ctx);
    let classes = classify_symbols(ctx, &indexed);
    drop(t);

    // Local symbols, then the debugger's notes: N_AST paths and stabs.
    let pexts: Vec<usize> =
        (0..classes.len()).into_par_iter().filter(|&i| classes[i] == SymbolClass::Pext).collect();
    let t = ctx.timer("symtab-locals");
    let locals = plan_local_symbols(ctx, &pexts);
    drop(t);

    // Debug stabs (see plan_stabs).
    let t = ctx.timer("symtab-stabs");
    let planned = plan_stabs(ctx);
    drop(t);

    let t = ctx.timer("symtab-entries");
    // Undefined (imported) symbols, sorted by name.
    let mut undefs: Vec<usize> =
        (0..classes.len()).into_par_iter().filter(|&i| classes[i] == SymbolClass::Undef).collect();
    undefs.par_sort_unstable_by_key(|&i| name_sort_key(ctx.symbols[i].name()));

    // Every range's size is known now: the entries and their names are
    // allocated once, and each range is filled in parallel. The names
    // are the strings layout_strings lays out below. The debug notes
    // are not among them: copy_symtab writes them from their plans.
    let nstabs: usize = planned.iter().map(|plan| plan.len()).sum();
    let nglobals = sorted_globals.len();
    let total = locals.len() + ast_paths(ctx).len() + sorted_globals.len() + undefs.len();
    let mut names: Vec<&'static [u8]> = Vec::with_capacity(total);
    data.entries.reserve_exact(total);

    par_push_entries(&mut names, &mut data.entries, &locals, |&ent| ent);
    let nplain = data.entries.len();
    drop(locals);

    push_ast_paths(ctx, &mut names, &mut data.entries);

    let stabs_start = data.entries.len();
    let nlocal = stabs_start + nstabs;
    data.nlocal = nlocal as u32;

    // Defined global symbols, sorted by name; the caller sorted them
    // once for this table and the export trie both.
    par_push_entries(&mut names, &mut data.entries, sorted_globals, |&i| global_entry(ctx, i));
    data.nextdef = sorted_globals.len() as u32;
    par_push_entries(&mut names, &mut data.entries, &undefs, |&i| import_entry(ctx, i));
    data.nundef = undefs.len() as u32;
    debug_assert_eq!(data.entries.len(), total);
    drop(t);

    // The string table: the entries' names, then each object's notes'
    // in a block of its own.
    let t = ctx.timer("symtab-strings");
    let strtab_end = layout_strings(&mut data.entries, &names);
    data.names = names;

    // Each symbol's index, for the indirect symbol table, and its string,
    // which the notes naming it share.
    let nsyms = ctx.symbols.syms.len();
    let entry_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    let strx_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    (0..nplain).into_par_iter().chain(stabs_start..stabs_start + nglobals).for_each(|i| {
        if let (ent, Some(id)) = data.entries[i] {
            let index = if i < stabs_start { i } else { i + nstabs };
            entry_of[id as usize].store(index as u32, Ordering::Relaxed);
            strx_of[id as usize].store(ent.stroff.get(), Ordering::Relaxed);
        }
    });
    undefs.par_iter().enumerate().for_each(|(k, &id)| {
        entry_of[id].store((nlocal + nglobals + k) as u32, Ordering::Relaxed);
    });
    data.output_sym_indices = entry_of.into_iter().map(AtomicU32::into_inner).collect();
    data.strx_of = strx_of.into_iter().map(AtomicU32::into_inner).collect();
    data.set_stabs(ctx, planned, stabs_start, strtab_end);

    make_indirect_aliases(ctx, &mut data);
    drop(t);

    data
}

/// The imports the image points at by their symbol table index, by
/// symbol: those of the stubs, GOT slots and lazy pointers (the
/// indirect symbol table) and the N_INDR aliases' targets. These are
/// listed whether or not live code uses them.
fn indexed_imports<E: Target>(ctx: &Context<E>) -> Vec<bool> {
    let mut indexed = vec![false; ctx.symbols.syms.len()];
    let slots = ctx
        .stubs
        .symbols
        .iter()
        .chain(&ctx.got.got_syms)
        .copied()
        .chain(ctx.objc_stubs.msgsend_sym)
        .chain(ctx.stub_helper.dyld_stub_binder)
        .chain(ctx.indirect_aliases.iter().map(|&(_, target)| target));
    for id in slots {
        indexed[id as usize] = true;
    }
    indexed
}

/// Where a symbol goes in the symbol table besides the defined globals
/// and its object's own locals: among the locals as a private external,
/// among the imports, or nowhere.
#[derive(Clone, Copy, PartialEq)]
enum SymbolClass {
    No,
    Pext,
    Undef,
}

/// Classifies every symbol (see SymbolClass) in one parallel pass,
/// instead of a full scan over millions of slots for each class. An
/// import is listed if live code or data uses it (dead stripping
/// refreshes which do, see dead_strip::mark_live_references) or the
/// image points at it by index (`indexed`), as mold-rust lists the
/// imports it uses. (ld-prime also lists, unbound, the imports only
/// bitcode or code its own dead stripping removed referred to, and the
/// imports of a merged mergeable library.)
fn classify_symbols<E: Target>(ctx: &Context<E>, indexed: &[bool]) -> Vec<SymbolClass> {
    (0..ctx.symbols.syms.len())
        .into_par_iter()
        .map(|i| {
            let sym = &ctx.symbols[i];
            if matches!(sym.file(), Some(FileId::Dylib(_))) {
                return if sym.is_used() || indexed[i] {
                    SymbolClass::Undef
                } else {
                    SymbolClass::No
                };
            }
            // A private external in a live object: a definition in a
            // live subsection, or a sectionless one - an absolute
            // symbol (N_ABS) or a hidden __mh_execute_header, which
            // ld64 keeps as locals too, but not a hidden -alias of an
            // import, which it drops.
            if sym.is_extern()
                && sym.is_private_extern()
                && matches!(sym.file(), Some(FileId::Obj(o)) if ctx.objs[o as usize].is_reachable)
                && match sym.input_section() {
                    Some(isec) => ctx.isecs[ctx.isecs.resolve(isec as usize)].is_alive(),
                    None => !ctx.indirect_aliases.iter().any(|&(a, _)| a == i as u32),
                }
            {
                // A private external becomes a local, and a label
                // is not emitted (clang's __OBJC_LABEL_PROTOCOL_$_X is
                // listed, demoted, but not an l_OBJC_LABEL_PROTOCOL_$_X).
                if !should_write_to_local_symtab(sym.name()) {
                    return SymbolClass::No;
                }
                return SymbolClass::Pext;
            }
            SymbolClass::No
        })
        .collect()
}

/// A defined global's entry, its name and the symbol whose address
/// fills in `value`.
fn global_entry<E: Target>(ctx: &Context<E>, i: SymbolId) -> NamedEntry {
    let sym = &ctx.symbols[i];
    let (n_type, sect, mut desc) = match (sym.file(), sym.input_section()) {
        (_, Some(isec)) => {
            (N_SECT | N_EXT, ctx.isecs[ctx.isecs.resolve(isec as usize)].sect_idx(ctx), 0)
        }
        // A synthesized symbol with no section (__mh_execute_header)
        // sits in the first section: the mach header. Nothing slides
        // a -static image without -pie, and there it is absolute, in no
        // section. (ld-prime keeps the section number.)
        (Some(FileId::Obj(o)), None) if ctx.is_internal(o as usize) => {
            if ctx.args.static_link && !ctx.args.pie {
                (N_ABS | N_EXT, 0, REFERENCED_DYNAMICALLY)
            } else {
                (N_SECT | N_EXT, 1, REFERENCED_DYNAMICALLY)
            }
        }
        (_, None) => (N_ABS | N_EXT, 0, 0),
    };
    if sym.is_weak_def() {
        desc |= N_WEAK_DEF;
    }
    if sym.is_referenced_dynamically() {
        desc |= REFERENCED_DYNAMICALLY;
    }
    let ent =
        MachSym { stroff: U32::new(0), n_type, sect, desc: U16::new(desc), value: U64::new(0) };
    (sym.name(), ent, Some(i))
}

/// An import's entry and its name. The library ordinal lives in the
/// high byte of `desc`.
fn import_entry<E: Target>(ctx: &Context<E>, i: usize) -> NamedEntry {
    let sym = &ctx.symbols[i];
    let Some(FileId::Dylib(_)) = sym.file() else { unreachable!() };
    // A dynamic-lookup import records the DYNAMIC_LOOKUP ordinal, a
    // -bundle_loader import the EXECUTABLE ordinal.
    let ordinal = msym_library_ordinal(ctx, sym) as u16;
    let mut desc = ordinal << 8;
    if sym.is_weak_ref() {
        desc |= N_WEAK_REF;
    }
    let ent = MachSym {
        stroff: U32::new(0),
        n_type: N_UNDF | N_EXT,
        sect: 0,
        desc: U16::new(desc),
        value: U64::new(0),
    };
    (sym.name(), ent, None)
}

/// The library ordinal in an undefined symbol's desc, which only
/// a two-level namespace image has: EXECUTABLE_ORDINAL (0xff) for
/// the -bundle_loader executable, DYNAMIC_LOOKUP_ORDINAL (0xfe) for
/// a symbol left to dynamic lookup, else the dylib's. ld-prime
/// writes 0 in a -flat_namespace image, where every import is a
/// flat lookup.
fn msym_library_ordinal<E: Target>(ctx: &Context<E>, sym: &Symbol) -> u8 {
    if ctx.args.flat_namespace {
        return 0;
    }
    match sym.bind_ordinal(ctx) {
        BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE => 0xff,
        n => n as u8,
    }
}

/// Makes each alias of an imported symbol an N_INDR entry whose value
/// is the string-table offset of the name it stands for: the string of
/// the import's own entry. (ld-prime writes the name again after the
/// alias's.) The slot is detached from the symbol so copy_symtab leaves
/// `value` alone.
fn make_indirect_aliases<E: Target>(ctx: &Context<E>, data: &mut SymtabSection) {
    let (stabs_start, nstabs) = (data.stabs_start, data.nstabs);
    let entry = |index: u32| {
        let index = index as usize;
        if index < stabs_start { index } else { index - nstabs }
    };
    for &(alias, target) in &ctx.indirect_aliases {
        let a = data.output_sym_indices[alias as usize];
        let t = data.output_sym_indices[target as usize];
        if a == u32::MAX || t == u32::MAX {
            continue;
        }
        let target_strx = data.entries[entry(t)].0.stroff;
        let ent = &mut data.entries[entry(a)];
        ent.0.n_type = N_INDR | N_EXT;
        ent.0.sect = 0;
        ent.0.desc.set(0);
        ent.0.value.set(target_strx.get() as u64);
        ent.1 = None;
    }
}

pub fn copy_symtab<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let symtab = &ctx.symtab;
    let symoff = symtab.hdr.fileoff as usize;
    let stroff = ctx.strtab.hdr.fileoff as usize;
    let symsize = symtab.len() * size_of::<MachSym>();
    let (syms, strtab) = if symoff + symsize <= stroff {
        let (lo, hi) = buf.split_at_mut(stroff);
        (&mut lo[symoff..symoff + symsize], &mut hi[..symtab.strtab_size])
    } else {
        let (lo, hi) = buf.split_at_mut(symoff);
        (&mut hi[..symsize], &mut lo[stroff..stroff + symtab.strtab_size])
    };
    copy_buf(ctx, symtab, syms, strtab);
}

/// Writes a symbol table's entries into `syms` and their strings into
/// `strtab`, which hold exactly the table and its strings: each entry
/// with the address of its symbol, if it has one, as `value`, and each
/// object's debug notes from its plan. A -r output's table is written
/// this way too.
pub fn copy_buf<E: Target>(
    ctx: &Context<E>,
    symtab: &SymtabSection,
    syms: &mut [u8],
    strtab: &mut [u8],
) {
    // The entries before the notes, the notes, and the externals; the
    // entries' strings, then the notes'.
    let start = symtab.stabs_start;
    let (local_syms, rest) = syms.split_at_mut(start * size_of::<MachSym>());
    let (mut stab_syms, extern_syms) = rest.split_at_mut(symtab.nstabs * size_of::<MachSym>());
    let stab_strx = symtab.stab_strx.first().map_or(strtab.len(), |&strx| strx as usize);
    let (strtab, mut stab_strtab) = strtab.split_at_mut(stab_strx);

    // Each object's notes are a block of the table of its own, with its
    // strings, carved off in order and written in parallel - mold-rust's
    // symtab copy_buf and populate_symtab.
    let mut blocks = Vec::with_capacity(symtab.stabs.len());
    for (plan, strx) in symtab.stabs.iter().zip(symtab.stab_strx.windows(2)) {
        let (syms, rest) = stab_syms.split_at_mut(plan.len() * size_of::<MachSym>());
        let (strs, strs_rest) = stab_strtab.split_at_mut((strx[1] - strx[0]) as usize);
        (stab_syms, stab_strtab) = (rest, strs_rest);
        blocks.push(SymtabBlock::new(syms, strs, strx[0]));
    }
    let stabs = || {
        symtab.stabs.par_iter().zip(blocks).for_each(|(plan, mut block)| {
            plan.populate_symtab(ctx, &symtab.strx_of, &mut block);
        });
    };

    // The string table opens with " \0" (offset 1 is the empty string);
    // every other string is written with its entry, straight into the
    // output. Each owns a disjoint range, and its NUL is already zero.
    strtab[..2].copy_from_slice(b" \0");
    struct BufPtr(*mut u8);
    unsafe impl Sync for BufPtr {}
    let strtab = BufPtr(strtab.as_mut_ptr());
    let strtab = &strtab;

    // Millions of entries, each wanting an addr lookup for its
    // value: emit them in parallel blocks.
    const BLOCK: usize = 4096;
    let (locals, externs) = symtab.entries.split_at(start);
    let (local_names, extern_names) = symtab.names.split_at(start);
    let write = |syms: &mut [u8], ents: &[(MachSym, Option<SymbolId>)], names: &[&[u8]]| {
        syms.par_chunks_mut(BLOCK * size_of::<MachSym>())
            .zip(ents.par_chunks(BLOCK))
            .zip(names.par_chunks(BLOCK))
            .for_each(|((out, ents), names)| {
                for (i, ((msym, sym), name)) in ents.iter().zip(names).enumerate() {
                    let mut msym = *msym;
                    if let Some(id) = sym {
                        msym.value.set(ctx.symbols[*id].addr(ctx));
                    }
                    msym.write(&mut out[i * size_of::<MachSym>()..]);
                    // SAFETY: layout_strings gave each name a range of
                    // its own within the string table.
                    unsafe {
                        let dst = strtab.0.add(msym.stroff.get() as usize);
                        std::ptr::copy_nonoverlapping(name.as_ptr(), dst, name.len());
                    }
                }
            });
    };
    let entries = || {
        rayon::join(
            || write(local_syms, locals, local_names),
            || write(extern_syms, externs, extern_names),
        )
    };
    rayon::join(entries, stabs);
}

/// Lays out a symbol table's strings and sets every entry's stroff:
/// each entry's name in entry order, with a copy of its own - two
/// locals of one name get two - but that an empty name is offset 1,
/// after the table's leading " ". The debug notes' follow (see
/// SymtabSection::set_stabs). Returns where the strings end.
pub fn layout_strings(entries: &mut [(MachSym, Option<SymbolId>)], names: &[&[u8]]) -> usize {
    // Each block of entries at its prefix-summed offset.
    const CHUNK: usize = 1 << 16;
    let size = |name: &&[u8]| if name.is_empty() { 0 } else { name.len() as u32 + 1 };
    let sums: Vec<u32> = names.par_chunks(CHUNK).map(|c| c.iter().map(size).sum()).collect();
    let mut bases = Vec::with_capacity(sums.len());
    let mut total = 2u32;
    for sum in sums {
        bases.push(total);
        total += sum;
    }
    entries.par_chunks_mut(CHUNK).zip(names.par_chunks(CHUNK)).zip(bases).for_each(
        |((ents, names), mut off)| {
            for ((ent, _), name) in ents.iter_mut().zip(names) {
                ent.stroff.set(if name.is_empty() { 1 } else { off });
                off += size(name);
            }
        },
    );
    total as usize
}
