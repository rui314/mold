//! The symbol table in __LINKEDIT: the symbols it lists, with the debug
//! notes (stabs) of each object, and the writer that emits it together
//! with the string table.

use rayon::prelude::*;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::input_files::{FileId, ObjectFile};
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;
use crate::util::{leak_bytes, path_bytes};

/// The symbol table, laid out before addresses are known. The symbol
/// slot of each entry supplies its final `n_value` when the table is
/// copied to the output.
#[derive(Debug)]
pub struct SymtabSection {
    pub hdr: ChunkHeader,
    /// The entries but the debug notes: those before the notes - the
    /// plain locals and N_AST paths - then the externals and imports,
    /// which follow the notes in the table.
    pub entries: Vec<(NList, Option<SymbolId>)>,
    /// The string table's total size (bytes, padded to 8). The bytes
    /// themselves are not materialized here: copy_symtab writes each
    /// entry's name at its n_strx straight into the output.
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
        Self {
            hdr: ChunkHeader::linkedit(),
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
    entries: &mut Vec<(NList, Option<SymbolId>)>,
) {
    for path in ast_paths(ctx) {
        names.push(leak_bytes(path_bytes(path).to_vec()));
        entries.push((NList { n_strx: 0, n_type: N_AST, ..Default::default() }, None));
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

/// Returns true if a local symbol should appear in the output symbol
/// table. Assembler temporaries, which begin with 'l' or 'L', are
/// dropped. (ld-prime also drops the names of the entries of the
/// Objective-C lists and of the sections it splits by content, which
/// are no temporaries: Swift's _objc_classes_*, clang's
/// __unnamed_array_storage.)
pub(crate) fn keep_local_symbol(name: &[u8]) -> bool {
    !name.is_empty() && !name.starts_with(b"l") && !name.starts_with(b"L")
}

/// One stab entry: its name and nlist, the symbol whose final address
/// fills in n_value, and the symbol the name is, if any, whose string
/// the entry shares.
#[derive(Clone, Copy, Debug)]
pub struct Stab {
    pub name: &'static [u8],
    pub ent: NList,
    pub value_of: Option<SymbolId>,
    pub name_of: Option<SymbolId>,
}

impl Stab {
    fn new(name: &'static [u8], ent: NList, value_of: Option<SymbolId>) -> Self {
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
        block: &mut crate::chunks::symtab::SymtabBlock<'_>,
    ) {
        // A function's notes take its address three times in a row.
        let mut addr = (u32::MAX, 0);
        for stab in self.stabs(ctx) {
            let mut ent = stab.ent;
            if let Some(id) = stab.value_of {
                if addr.0 != id {
                    addr = (id, ctx.sym_addr(id));
                }
                ent.n_value = addr.1;
            }
            // A nameless entry gets the empty string after the string
            // table's leading space.
            ent.n_strx = match stab.shared_strx(strx_of) {
                Some(strx) => strx,
                None if stab.name.is_empty() => 1,
                None => block.add_string(stab.name),
            };
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
    n_sect: u8,
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
        let sect = self.n_sect;
        // Named entries get their string offsets later; the rest keep 1,
        // the empty string.
        let stab = |n_type, n_sect| NList { n_strx: 1, n_type, n_sect, ..Default::default() };
        let mut out = [Stab::new(b"", stab(N_BNSYM, sect), id); 4];
        match self.n_type {
            N_FUN => {
                // ld64's shape: N_BNSYM, the N_FUN pair (the function's
                // address, then its size), N_ENSYM. Its stab reader takes
                // an N_FUN without the bracketing symbols badly (a crash
                // on a -r output that had only the pair).
                let fun = NList { n_strx: 0, ..stab(N_FUN, sect) };
                out[1] = Stab { name_of: id, ..Stab::new(name, fun, id) };
                out[2] =
                    Stab::new(b"", NList { n_value: self.size as u64, ..stab(N_FUN, 0) }, None);
                out[3] = Stab::new(b"", stab(N_ENSYM, sect), id);
            }
            // An N_GSYM names the global only, with no section or
            // address - the debugger looks the address up by name.
            N_GSYM => {
                let ent = NList { n_type: N_GSYM, ..Default::default() };
                out[0] = Stab { name, ent, value_of: None, name_of: id };
            }
            _ => {
                let ent = NList { n_strx: 0, ..stab(N_STSYM, sect) };
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
    if !obj.is_alive {
        return StabPlan::default();
    }
    if obj.nlists.iter().any(|n| n.n_type == N_OSO) {
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
    for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
        let sym = &ctx.symbols[sym_id];
        // A tentative definition gets its note in each object that
        // declares it. A global with an assembler-local name (Swift's
        // l_OBJC_PROTOCOL_SYMREF_$_*, weak private externals) gets none,
        // as no local of that name does.
        let common = nlist.is_common() && is_still_common(ctx, sym_id);
        if nlist.is_stab()
            || (!common && !matches!(sym.file(), Some(FileId::Obj(o)) if o as usize == obj_idx))
            || !keep_local_symbol(sym.name())
        {
            continue;
        }
        plan.syms.extend(symbol_stabs(ctx, obj, sym_id, nlist, common));
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
    // Entries whose n_value is an address in the object (n_sect
    // says which section); an N_FUN with an empty name holds the
    // function's size instead.
    let addressed = |n: &NList| {
        n.n_sect != 0
            && matches!(
                n.n_type,
                N_FUN | N_BNSYM | N_ENSYM | N_STSYM | N_LCSYM | N_SLINE | N_ECOMM | N_ECOML
            )
    };
    // The object's own local symbols by name, for the notes that
    // name them.
    let r = obj.local_range();
    let locals: hashbrown::HashMap<&[u8], (SymbolId, &NList)> = obj.nlists[r.clone()]
        .iter()
        .zip(&obj.symbols[r])
        .filter(|(n, _)| !n.is_stab())
        .map(|(n, &id)| (ctx.symbols[id].name(), (id, n)))
        .collect();
    let mut skip_size = false;
    let mut in_unit = false;
    for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
        if !nlist.is_stab() {
            continue;
        }
        let mut ent = *nlist;
        let name = ctx.symbols[sym_id].name();
        // A closing N_SO before the first unit (ld-prime's -r outputs
        // open their stabs with one) closes nothing: it is not copied.
        if nlist.n_type == N_SO && name.is_empty() {
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
        ent.n_strx = if name.is_empty() { 1 } else { 0 };
        // -reproducible (or ZERO_AR_DATE) zeroes the modification time
        // the earlier link wrote, as it does an object's own.
        if nlist.n_type == N_OSO && ctx.args.zero_ar_date {
            ent.n_value = 0;
        }
        if nlist.n_type == N_GSYM {
            out.extend(copy_global_stab(ctx, obj_idx, name, ent, &locals));
            continue;
        }
        if addressed(nlist) {
            let Some((isec, off)) = noted_subsec(ctx, obj, nlist.n_sect, nlist.n_value) else {
                // Dead code: drop the note, and a function's size
                // entry with it.
                skip_size = nlist.n_type == N_FUN;
                continue;
            };
            ent.n_value = ctx.isec_addr(isec) + off;
            ent.n_sect = ctx.isec_n_sect(&ctx.isecs[isec]);
        } else if nlist.n_type == N_FUN && skip_size {
            skip_size = false;
            continue;
        }
        let name_of = match nlist.n_type {
            N_FUN | N_STSYM | N_LCSYM if !name.is_empty() => {
                locals.get(name).map(|&(id, _)| id).or_else(|| ctx.symbols.get(name))
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
    ent: NList,
    locals: &hashbrown::HashMap<&[u8], (SymbolId, &NList)>,
) -> Option<Stab> {
    let obj = &ctx.objs[obj_idx];
    if let Some(&(id, nlist)) = locals.get(name) {
        let n_sect = if nlist.n_type() == N_ABS {
            0
        } else {
            let (isec, _) = noted_subsec(ctx, obj, nlist.n_sect, nlist.n_value)?;
            ctx.isec_n_sect(&ctx.isecs[isec])
        };
        let ent = NList { n_type: N_STSYM, n_sect, ..ent };
        return Some(Stab { name, ent, value_of: Some(id), name_of: Some(id) });
    }
    let id = ctx.symbols.get(name)?;
    let own = matches!(ctx.symbols[id].file(), Some(FileId::Obj(o)) if o as usize == obj_idx);
    (own || is_still_common(ctx, id)).then(|| {
        let ent = NList { n_sect: 0, n_value: 0, ..ent };
        Stab { name, ent, value_of: None, name_of: Some(id) }
    })
}

/// The live subsection holding an object's symbol or note at
/// `n_sect`/`n_value`, with the offset in it, unless it was coalesced
/// away (see is_coalesced_away): ld-prime notes the survivor's
/// symbols only.
fn noted_subsec<E: Target>(
    ctx: &Context<E>,
    obj: &ObjectFile,
    n_sect: u8,
    n_value: u64,
) -> Option<(usize, u64)> {
    let (isec, off) =
        crate::input_files::find_symbol_subsec(&ctx.isecs, &obj.subsecs, n_sect, n_value)?;
    if is_coalesced_away(ctx, isec) {
        return None;
    }
    let isec = ctx.resolve_isec(isec);
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
            let leaf = obj.mf.name.file_name().map_or(&[][..], |f| f.as_bytes());
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
        out.push(Stab::new(name, NList { n_type: N_SO, ..Default::default() }, None));
    }
    let path = match obj.mf.parent {
        Some(parent) if parent.name.is_absolute() => obj.mf.name.clone(),
        Some(_) | None if obj.mf.name.is_absolute() => obj.mf.name.clone(),
        // An object ThinLTO compiled in memory has no name, here either.
        None if obj.mf.name.as_os_str().is_empty() => std::path::PathBuf::new(),
        _ => cwd.join(&obj.mf.name),
    };
    let path = crate::input_files::without_fat_arch(path_bytes(&path));
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
    // n_value is the object's modification time, which dsymutil and
    // lldb compare against the file they find (0 disables the check):
    // an archive member's is its header's, and the LTO object's 0
    // unless -object_path_lto wrote it out. ZERO_AR_DATE, set to
    // anything, or -reproducible zeroes them all.
    let mtime = if ctx.args.zero_ar_date {
        0
    } else if let Some(date) = obj.mf.mtime {
        date
    } else {
        std::fs::metadata(crate::util::os_str(&path))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs())
    };
    out.push(Stab::new(
        leak_bytes(oso_name),
        NList { n_strx: 0, n_type: N_OSO, n_sect: E::CPUSUBTYPE as u8, n_desc: 1, n_value: mtime },
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
    nlist: &NList,
    common: bool,
) -> Option<SymbolStabs> {
    let sym = &ctx.symbols[sym_id];
    let global = SymbolStabs { sym: sym_id, size: 0, n_sect: 0, n_type: N_GSYM };
    let Some(isec) = sym.input_section().map(|i| i as usize) else {
        return common.then_some(global);
    };
    // The symbol has moved to the survivor if its subsection was
    // coalesced away; its own is the one to look at.
    if nlist.n_type() == N_SECT && noted_subsec(ctx, obj, nlist.n_sect, nlist.n_value).is_none() {
        return None;
    }
    let isec = &ctx.isecs[ctx.resolve_isec(isec)];
    let hdr = ctx.hdr_of(isec);
    if !isec.is_alive() {
        return None;
    }
    let n_sect = ctx.isec_n_sect(isec);
    let is_text = hdr.segname_is(b"__TEXT")
        && hdr.flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0;
    Some(if is_text {
        SymbolStabs { size: isec.size, n_sect, n_type: N_FUN, ..global }
    } else if nlist.is_extern() {
        global
    } else {
        SymbolStabs { n_sect, n_type: N_STSYM, ..global }
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
pub const STAB_END: NList = NList { n_strx: 1, n_type: N_SO, n_sect: 1, n_desc: 0, n_value: 0 };

/// A final image's local symbols: each object's non-external symbols
/// it keeps, in its symbol table's order, then the private externals
/// demoted to locals, then the linker's own names and the objc_msgSend$
/// stubs. -x drops them all, the demoted private externals too, as ld64
/// lists no local symbol under it. (ld-prime lists them by address, the
/// names at one address by rank, a subsection's own name last.)
fn plan_local_symbols<E: Target>(ctx: &Context<E>, pexts: &[usize]) -> Vec<NamedEntry> {
    if ctx.args.strip_locals {
        return Vec::new();
    }
    let per_obj: Vec<Vec<NamedEntry>> =
        ctx.objs.par_iter().map(|obj| object_locals(ctx, obj)).collect();
    let mut ents = per_obj.concat();

    // Private external symbols resolve globally but appear as locals
    // (with N_PEXT still set) in the output.
    for &i in pexts {
        let sym = &ctx.symbols[i];
        let id = Some(i as SymbolId);
        let (ent, id) = match (sym.file(), sym.input_section()) {
            (_, Some(isec)) => {
                let isec = &ctx.isecs[ctx.resolve_isec(isec as usize)];
                (NList { n_type: N_SECT | N_PEXT, ..local_nlist(ctx.isec_n_sect(isec), 0) }, id)
            }
            // A hidden __mh_execute_header (an export list that omits
            // it, or -no_exported_symbols) sits in the first section,
            // the mach header.
            (Some(FileId::Obj(o)), None) if ctx.is_internal(o as usize) => {
                (NList { n_type: N_SECT | N_PEXT, ..local_nlist(1, 0) }, id)
            }
            (_, None) => (NList { n_type: N_ABS | N_PEXT, ..local_nlist(0, sym.value) }, None),
        };
        // A demoted weak definition keeps N_WEAK_DEF.
        let n_desc = if sym.is_weak_def() { N_WEAK_DEF } else { 0 };
        ents.push((local_symbol_name(sym.name()), NList { n_desc, ..ent }, id));
    }
    ents.extend(linker_locals(ctx));
    ents
}

/// The local names the linker gives its own code and data: on
/// synthesized data, the objc_msgSend$ stubs, the lazy-load helpers and
/// slots, and the range-extension thunks' entries.
fn linker_locals<E: Target>(ctx: &Context<E>) -> Vec<NamedEntry> {
    let mut ents = Vec::new();
    // Locals the linker named itself, on synthesized data whose
    // addresses are final by now.
    for &(name, isec) in &ctx.extra_local_syms {
        let sec = &ctx.isecs[isec as usize];
        if sec.is_alive() && sec.output_section().is_some() {
            let addr = ctx.isec_addr(isec as usize);
            let ent = local_nlist(ctx.isec_n_sect(sec), addr);
            ents.push((name, ent, None));
        }
    }
    // The selector stubs, each a non-external symbol with N_PEXT
    // set (nm: "was a private external"), as ld64 lists them -
    // NetNewsWire's debug dylib has 851 _objc_msgSend$... entries.
    let hdr = &ctx.objc_stubs.hdr;
    for (i, &(sym, _)) in ctx.objc_stubs.symbols.iter().enumerate() {
        let addr = hdr.addr + i as u64 * ctx.objc_stub_size();
        let ent = NList { n_type: N_PEXT | N_SECT, ..local_nlist(hdr.n_sect, addr) };
        ents.push((ctx.symbols[sym].name(), ent, None));
    }
    // The lazy-load helpers - a call helper, like a selector stub,
    // with N_PEXT set - and slots.
    let hdr = &ctx.lazy_helpers.hdr;
    for (i, h) in ctx.lazy_helpers.helpers.iter().enumerate() {
        let addr = ctx.lazy_helper_addr(i);
        let n_type = match h.kind {
            crate::chunks::lazy_helpers::LazyUse::Call => N_PEXT | N_SECT,
            _ => N_SECT,
        };
        let ent = NList { n_type, ..local_nlist(hdr.n_sect, addr) };
        ents.push((h.name, ent, None));
    }
    let hdr = &ctx.lazy_load_got.hdr;
    for (i, &(_, name)) in ctx.lazy_load_got.slots.iter().enumerate() {
        let addr = hdr.addr + i as u64 * 8;
        ents.push((name, local_nlist(hdr.n_sect, addr), None));
    }
    // The delay-init stubs, like selector stubs with N_PEXT set, and
    // the helpers. (The dlopen helpers' flags are extra_local_syms.)
    let delay = &ctx.delay_init;
    for (i, stub) in delay.stubs.iter().enumerate() {
        let addr = ctx.delay_stub_addr(i);
        let ent = NList { n_type: N_PEXT | N_SECT, ..local_nlist(delay.stubs_hdr.n_sect, addr) };
        ents.push((stub.name, ent, None));
    }
    let n_sect = delay.helper_hdr.n_sect;
    for (i, h) in delay.helpers.iter().enumerate() {
        let addr = ctx.delay_helper_addr(i);
        ents.push((h.name, local_nlist(n_sect, addr), None));
    }
    for (i, d) in delay.dlopens.iter().enumerate() {
        let addr = ctx.dlopen_helper_addr(i);
        ents.push((d.name, local_nlist(n_sect, addr), None));
    }
    // The range-extension thunks' entries, named as ld-prime names
    // its branch islands.
    for (addr, n_sect, name) in crate::thunks::island_symbols(ctx) {
        if !is_listed_out(ctx, name) {
            ents.push((name, local_nlist(n_sect, addr), None));
        }
    }
    ents
}

/// A local symbol's entry, in section `n_sect`.
fn local_nlist(n_sect: u8, n_value: u64) -> NList {
    NList { n_strx: 0, n_type: N_SECT, n_sect, n_desc: 0, n_value }
}

/// Whether -non_global_symbols_no_strip_list or -non_global_symbols_strip_list
/// filters out a local symbol, by name. (Stabs are unaffected.)
pub(crate) fn is_listed_out<E: Target>(ctx: &Context<E>, name: &[u8]) -> bool {
    ctx.args.local_keep_list.as_ref().is_some_and(|keep| keep.find(name) == -1)
        || ctx.args.local_strip_list.find(name) != -1
}

/// The non-external symbols of an object that the output lists, in
/// symbol-table order.
fn object_locals<E: Target>(ctx: &Context<E>, obj: &ObjectFile) -> Vec<NamedEntry> {
    let mut out = Vec::new();
    if !obj.is_alive {
        return out;
    }
    for i in obj.local_range() {
        let (nlist, sym_id) = (&obj.nlists[i], obj.symbols[i]);
        let sym = &ctx.symbols[sym_id];
        if nlist.is_stab() || nlist.is_extern() || !keep_local_symbol(sym.name()) {
            continue;
        }
        if is_listed_out(ctx, sym.name()) {
            continue;
        }
        // An absolute symbol (N_ABS, as `.set x, 5` makes) is kept too,
        // in no section.
        let Some(isec) = sym.input_section().map(|i| i as usize) else {
            if nlist.n_type() == N_ABS {
                let ent = NList { n_type: N_ABS, ..local_nlist(0, 0) };
                let name = local_symbol_name(sym.name());
                out.push((name, ent, Some(sym_id)));
            }
            continue;
        };
        // A folded function's name names the function it folded into.
        let kept = ctx.resolve_isec(isec);
        if !matches!(sym.file(), Some(FileId::Obj(_))) || !ctx.isecs[kept].is_alive() {
            continue;
        }
        let ent = local_nlist(ctx.isec_n_sect(&ctx.isecs[kept]), 0);
        let name = local_symbol_name(sym.name());
        out.push((name, ent, Some(sym_id)));
    }
    out
}

/// A symbol table entry with its name, and the symbol whose address
/// fills n_value.
type NamedEntry = (&'static [u8], NList, Option<SymbolId>);

/// Appends an entry and its name for each item, made by `f` on all cores
/// straight into the arrays' spare capacity, which the caller reserved.
pub fn par_push_entries<T: Sync>(
    names: &mut Vec<&'static [u8]>,
    entries: &mut Vec<(NList, Option<SymbolId>)>,
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
    undefs.par_sort_unstable_by_key(|&i| crate::util::name_sort_key(ctx.symbols[i].name()));

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
            strx_of[id as usize].store(ent.n_strx, Ordering::Relaxed);
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
                && matches!(sym.file(), Some(FileId::Obj(o)) if ctx.objs[o as usize].is_alive)
                && match sym.input_section() {
                    Some(isec) => ctx.isecs[ctx.resolve_isec(isec as usize)].is_alive(),
                    None => !ctx.indirect_aliases.iter().any(|&(a, _)| a == i as u32),
                }
            {
                // A private external becomes a local, and a label
                // is not emitted (clang's __OBJC_LABEL_PROTOCOL_$_X is
                // listed, demoted, but not an l_OBJC_LABEL_PROTOCOL_$_X).
                if !keep_local_symbol(sym.name()) {
                    return SymbolClass::No;
                }
                return SymbolClass::Pext;
            }
            SymbolClass::No
        })
        .collect()
}

/// A defined global's entry, its name and the symbol whose address
/// fills in n_value.
fn global_entry<E: Target>(ctx: &Context<E>, i: SymbolId) -> NamedEntry {
    let sym = &ctx.symbols[i];
    let (n_type, n_sect, mut n_desc) = match (sym.file(), sym.input_section()) {
        (_, Some(isec)) => {
            (N_SECT | N_EXT, ctx.isec_n_sect(&ctx.isecs[ctx.resolve_isec(isec as usize)]), 0)
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
        n_desc |= N_WEAK_DEF;
    }
    if sym.is_referenced_dynamically() {
        n_desc |= REFERENCED_DYNAMICALLY;
    }
    let ent = NList { n_strx: 0, n_type, n_sect, n_desc, n_value: 0 };
    (sym.name(), ent, Some(i))
}

/// An import's entry and its name. The library ordinal lives in the
/// high byte of n_desc.
fn import_entry<E: Target>(ctx: &Context<E>, i: usize) -> NamedEntry {
    let sym = &ctx.symbols[i];
    let Some(FileId::Dylib(dylib)) = sym.file() else { unreachable!() };
    // A dynamic-lookup import records the DYNAMIC_LOOKUP ordinal, a
    // -bundle_loader import the EXECUTABLE ordinal.
    let ordinal = ctx.nlist_library_ordinal(dylib) as u16;
    let mut n_desc = ordinal << 8;
    if sym.is_weak_ref() {
        n_desc |= N_WEAK_REF;
    }
    let ent = NList { n_strx: 0, n_type: N_UNDF | N_EXT, n_sect: 0, n_desc, n_value: 0 };
    (sym.name(), ent, None)
}

/// Makes each alias of an imported symbol an N_INDR entry whose n_value
/// is the string-table offset of the name it stands for: the string of
/// the import's own entry. (ld-prime writes the name again after the
/// alias's.) The slot is detached from the symbol so copy_symtab leaves
/// n_value alone.
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
        let target_strx = data.entries[entry(t)].0.n_strx;
        let ent = &mut data.entries[entry(a)];
        ent.0.n_type = N_INDR | N_EXT;
        ent.0.n_sect = 0;
        ent.0.n_desc = 0;
        ent.0.n_value = target_strx as u64;
        ent.1 = None;
    }
}

pub fn copy_symtab<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let symtab = &ctx.symtab;
    let symoff = symtab.hdr.fileoff as usize;
    let stroff = ctx.strtab.hdr.fileoff as usize;
    let symsize = symtab.len() * size_of::<NList>();
    let (syms, strtab) = if symoff + symsize <= stroff {
        let (lo, hi) = buf.split_at_mut(stroff);
        (&mut lo[symoff..symoff + symsize], &mut hi[..symtab.strtab_size])
    } else {
        let (lo, hi) = buf.split_at_mut(symoff);
        (&mut hi[..symsize], &mut lo[stroff..stroff + symtab.strtab_size])
    };
    write_symtab(ctx, symtab, syms, strtab);
}

/// Writes a symbol table's entries into `syms` and their strings into
/// `strtab`, which hold exactly the table and its strings: each entry
/// with the address of its symbol, if it has one, as n_value, and each
/// object's debug notes from its plan. A -r output's table is written
/// this way too.
pub fn write_symtab<E: Target>(
    ctx: &Context<E>,
    symtab: &SymtabSection,
    syms: &mut [u8],
    strtab: &mut [u8],
) {
    // The entries before the notes, the notes, and the externals; the
    // entries' strings, then the notes'.
    let start = symtab.stabs_start;
    let (local_syms, rest) = syms.split_at_mut(start * size_of::<NList>());
    let (mut stab_syms, extern_syms) = rest.split_at_mut(symtab.nstabs * size_of::<NList>());
    let stab_strx = symtab.stab_strx.first().map_or(strtab.len(), |&strx| strx as usize);
    let (strtab, mut stab_strtab) = strtab.split_at_mut(stab_strx);

    // Each object's notes are a block of the table of its own, with its
    // strings, carved off in order and written in parallel - mold-rust's
    // symtab copy_buf and populate_symtab.
    let mut blocks = Vec::with_capacity(symtab.stabs.len());
    for (plan, strx) in symtab.stabs.iter().zip(symtab.stab_strx.windows(2)) {
        let (syms, rest) = stab_syms.split_at_mut(plan.len() * size_of::<NList>());
        let (strs, strs_rest) = stab_strtab.split_at_mut((strx[1] - strx[0]) as usize);
        (stab_syms, stab_strtab) = (rest, strs_rest);
        blocks.push(SymtabBlock {
            syms,
            len: 0,
            strtab: strs,
            strtab_base: strx[0],
            strtab_len: 0,
        });
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

    // Millions of entries, each wanting a sym_addr lookup for its
    // n_value: emit them in parallel blocks.
    const BLOCK: usize = 4096;
    let (locals, externs) = symtab.entries.split_at(start);
    let (local_names, extern_names) = symtab.names.split_at(start);
    let write = |syms: &mut [u8], ents: &[(NList, Option<SymbolId>)], names: &[&[u8]]| {
        syms.par_chunks_mut(BLOCK * size_of::<NList>())
            .zip(ents.par_chunks(BLOCK))
            .zip(names.par_chunks(BLOCK))
            .for_each(|((out, ents), names)| {
                for (i, ((nlist, sym), name)) in ents.iter().zip(names).enumerate() {
                    let mut nlist = *nlist;
                    if let Some(id) = sym {
                        nlist.n_value = ctx.sym_addr(*id);
                    }
                    nlist.write_to(&mut out[i * size_of::<NList>()..]);
                    // SAFETY: layout_strings gave each name a range of
                    // its own within the string table.
                    unsafe {
                        let dst = strtab.0.add(nlist.n_strx as usize);
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

/// An object's block of the symbol table and of the string table, which
/// its debug notes are written into in place - mold-rust's SymtabBlock.
/// Blocks don't overlap, so they are written in parallel.
pub struct SymtabBlock<'a> {
    syms: &'a mut [u8],
    len: usize,
    strtab: &'a mut [u8],
    /// The offset of `strtab` within the string table.
    strtab_base: u32,
    strtab_len: usize,
}

impl SymtabBlock<'_> {
    #[inline]
    pub fn push(&mut self, nlist: NList) {
        nlist.write_to(&mut self.syms[self.len * size_of::<NList>()..]);
        self.len += 1;
    }

    /// Adds a string, returning its offset in the string table.
    #[inline]
    pub fn add_string(&mut self, name: &[u8]) -> u32 {
        let strx = self.strtab_base + self.strtab_len as u32;
        let strs = &mut self.strtab[self.strtab_len..];
        strs[..name.len()].copy_from_slice(name);
        strs[name.len()] = 0;
        self.strtab_len += name.len() + 1;
        strx
    }
}

/// Lays out a symbol table's strings and sets every entry's n_strx:
/// each entry's name in entry order, with a copy of its own - two
/// locals of one name get two - but that an empty name is offset 1,
/// after the table's leading " ". The debug notes' follow (see
/// SymtabSection::set_stabs). Returns where the strings end.
pub fn layout_strings(entries: &mut [(NList, Option<SymbolId>)], names: &[&[u8]]) -> usize {
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
                ent.n_strx = if name.is_empty() { 1 } else { off };
                off += size(name);
            }
        },
    );
    total as usize
}
