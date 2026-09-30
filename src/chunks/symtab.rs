//! The symbol table in __LINKEDIT: the symbols it lists, in ld-prime's
//! order, with the debug notes (stabs) of each object, and the writer
//! that emits it together with the string table.

use rayon::prelude::*;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::input_files::FileId;
use crate::macho::*;
use crate::objc::{DataField, ObjcRef};
use crate::passes::{has_unnamed_atoms, is_unnamed_objc_list, objc_list_aliases};
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
    /// plain locals, N_AST paths and the notes' opening N_SO - then the
    /// externals and imports, which follow the notes in the table.
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
}

impl Default for SymtabSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns true if a local symbol should appear in the output symbol
/// table. Assembler temporaries, which begin with 'l' or 'L', are
/// dropped.
fn keep_local_symbol(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('l') && !name.starts_with('L')
}

/// Returns true if a non-external local symbol defined in `isec`
/// appears in a final image's symbol table: its name must not be a
/// label, and it must not name the entry of an Objective-C list that
/// ld-prime names no symbol for (Swift's _objc_classes_* in
/// __objc_classlist: ld-prime's NetNewsWire has none of the 127 ours
/// carried) but as an alias (`list_alias`, see objc_list_aliases), nor
/// live in __objc_protolist or __objc_imageinfo. A demoted private
/// external in those two stays (clang's __OBJC_LABEL_PROTOCOL_$_X
/// does), as does one an earlier ld -r demoted, a local that kept
/// N_PEXT (`demoted`). A superclass or protocol reference keeps its
/// label, unless it is of the literal-pointer type (see
/// has_unnamed_atoms).
fn keep_local_symbol_in<E: Target>(
    ctx: &Context<E>,
    name: &str,
    isec: Option<u32>,
    demoted: bool,
    list_alias: bool,
) -> bool {
    if !keep_local_symbol(name) {
        return false;
    }
    let Some(isec) = isec else { return true };
    let isec = &ctx.isecs[ctx.resolve_isec(isec as usize)];
    if ctx.is_internal(isec.file as usize) {
        return true;
    }
    let hdr = ctx.hdr_of(isec);
    if is_unnamed_objc_list(hdr) {
        return list_alias;
    }
    if demoted {
        return true;
    }
    !(hdr.segname_is("__DATA")
        && (hdr.sectname_is("__objc_protolist") || hdr.sectname_is("__objc_imageinfo"))
        || has_unnamed_atoms(hdr))
}

/// Whether a symbol names a method list convert_objc_method_lists
/// rewrote in the relative form, in __TEXT,__objc_methlist.
fn names_relative_method_list<E: Target>(ctx: &Context<E>, id: crate::symbol::SymbolId) -> bool {
    ctx.symbols[id].input_section().is_some_and(|isec| {
        let hdr = ctx.hdr_of(&ctx.isecs[ctx.resolve_isec(isec as usize)]);
        hdr.segname_is("__TEXT") && hdr.sectname_is("__objc_methlist")
    })
}

/// One stab entry: its name and nlist, the symbol whose final address
/// fills in n_value, and the symbol the name is, if any - ld-prime
/// points the entry at that symbol's own string.
#[derive(Clone, Copy, Debug)]
pub struct Stab {
    pub name: &'static [u8],
    pub ent: NList,
    pub value_of: Option<crate::symbol::SymbolId>,
    pub name_of: Option<crate::symbol::SymbolId>,
}

impl Stab {
    fn new(name: &'static [u8], ent: NList, value_of: Option<crate::symbol::SymbolId>) -> Self {
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
        fixed.map(|s| s.name.len() + 1).sum::<usize>()
            + syms.map(|s| ctx.symbols[s.sym].name().len() + 1).sum::<usize>()
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
    sym: crate::symbol::SymbolId,
    size: u32,
    n_sect: u8,
    n_type: u8,
}

impl SymbolStabs {
    fn len(&self) -> usize {
        if self.n_type == N_FUN { 4 } else { 1 }
    }

    fn stabs<E: Target>(&self, ctx: &Context<E>) -> impl Iterator<Item = Stab> {
        let id = Some(self.sym);
        let name = ctx.symbols[self.sym].name().as_bytes();
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

/// Plans one object's debug-note stabs. An object with DWARF gets the
/// run ld64 writes: N_SO, N_OSO naming the object, N_FUN pairs and
/// N_GSYM/N_STSYM for its symbols, and a closing N_SO. An object that
/// already carries such a run (a -r output: ld64 does not merge DWARF,
/// it writes these notes) has it copied through, the address-bearing
/// entries rebased to their subsections' output addresses and those of
/// dead subsections dropped. Shared by the final link and -r.
pub fn plan_object_stabs<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
    cwd: &Path,
    commons: &hashbrown::HashMap<crate::symbol::SymbolId, usize>,
) -> StabPlan {
    let obj = &ctx.objs[obj_idx];
    let mut plan = StabPlan::default();
    if !obj.is_alive {
        return plan;
    }
    let out = &mut plan.fixed;

    if obj.nlists.iter().any(|n| n.n_type == N_OSO) {
        // Entries whose n_value is an address in the object (n_sect
        // says which section); an N_FUN with an empty name holds the
        // function's size instead.
        let addressed = |n: &NList| {
            n.n_sect != 0
                && matches!(
                    n.n_type,
                    N_FUN
                        | N_BNSYM
                        | N_ENSYM
                        | N_GSYM
                        | N_STSYM
                        | N_LCSYM
                        | N_SLINE
                        | N_ECOMM
                        | N_ECOML
                )
        };
        // The object's own local symbols by name, for the notes that
        // name them.
        let r = obj.local_range();
        let locals: hashbrown::HashMap<&str, crate::symbol::SymbolId> = obj.nlists[r.clone()]
            .iter()
            .zip(&obj.symbols[r])
            .filter(|(n, _)| !n.is_stab())
            .map(|(_, &id)| (ctx.symbols[id].name(), id))
            .collect();
        let mut skip_size = false;
        let mut in_unit = false;
        for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
            if !nlist.is_stab() {
                continue;
            }
            let mut ent = *nlist;
            let name = ctx.symbols[sym_id].name();
            // The closing N_SO that opens the input's stabs is not
            // copied: the output has its own.
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
            if addressed(nlist) {
                let placed = crate::input_files::find_symbol_subsec(
                    &ctx.isecs,
                    &obj.subsecs,
                    nlist.n_sect,
                    nlist.n_value,
                )
                .map(|(isec, off)| (ctx.resolve_isec(isec), off))
                .filter(|&(isec, _)| ctx.isecs[isec].is_alive());
                let Some((isec, off)) = placed else {
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
                N_FUN | N_STSYM | N_GSYM | N_LCSYM if !name.is_empty() => {
                    locals.get(name).copied().or_else(|| ctx.symbols.get(name))
                }
                _ => None,
            };
            out.push(Stab { name: name.as_bytes(), ent, value_of: None, name_of });
        }
        plan.len = plan.fixed.len();
        return plan;
    }

    if !obj.has_debug_info {
        return plan;
    }

    // ld64 opens each object's run with two N_SO entries, the
    // compilation directory (with a trailing slash) and the source
    // file, both from the DWARF compile unit; its own stab reader
    // takes an N_SO with an empty name as the closing one, so a -r
    // output without them crashed it. N_OSO then points at the
    // object (or "archive(member)"), as an absolute path.
    let (dir, file) = match crate::dwarf::compile_unit_name(obj.mf.data(), &obj.sect_hdrs) {
        Some((dir, file)) => (dir, file),
        None => {
            let leaf = obj.mf.name.file_name().map_or(&[][..], |f| f.as_bytes());
            (Vec::new(), leaf.to_vec())
        }
    };
    let mut dir = if dir.is_empty() { path_bytes(cwd).to_vec() } else { dir };
    if !dir.ends_with(b"/") {
        dir.push(b'/');
    }
    for name in [dir, file] {
        out.push(Stab::new(leak_bytes(name), NList { n_type: N_SO, ..Default::default() }, None));
    }
    let mut oso_name: Vec<u8> = match obj.mf.parent {
        Some(parent) if parent.name.is_absolute() => path_bytes(&obj.mf.name).to_vec(),
        Some(_) | None if obj.mf.name.is_absolute() => path_bytes(&obj.mf.name).to_vec(),
        _ => path_bytes(&cwd.join(&obj.mf.name)).to_vec(),
    };
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
    // an archive member's is its header's. ZERO_AR_DATE, set to
    // anything, zeroes them all for reproducible builds.
    let mtime = if ctx.args.zero_ar_date {
        0
    } else if let Some(date) = obj.mf.ar_date {
        date
    } else {
        std::fs::metadata(&obj.mf.name)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs())
    };
    out.push(Stab::new(
        leak_bytes(std::mem::take(&mut oso_name)),
        NList { n_strx: 0, n_type: N_OSO, n_sect: E::CPUSUBTYPE as u8, n_desc: 1, n_value: mtime },
        None,
    ));

    // The symbols' notes, in symbol-table order. ld-prime lists a
    // unit's notes by address instead, but no reader depends on that:
    // dsymutil and lldb map each unit's notes by name, and an N_FUN pair
    // stays together either way.
    let aliases = objc_list_aliases(ctx, obj);
    for (i, (nlist, &sym_id)) in obj.nlists.iter().zip(&obj.symbols).enumerate() {
        let sym = &ctx.symbols[sym_id];
        // A tentative definition gets its note in the first object that
        // declares it.
        let common = nlist.is_common() && commons.get(&sym_id) == Some(&obj_idx);
        if nlist.is_stab()
            || (!common && !matches!(sym.file(), Some(FileId::Obj(o)) if o as usize == obj_idx))
            || (!nlist.is_extern()
                && !keep_local_symbol_in(
                    ctx,
                    sym.name(),
                    sym.input_section(),
                    nlist.n_type & N_PEXT != 0,
                    aliases.contains(&i),
                ))
        {
            continue;
        }
        plan.syms.extend(symbol_stabs(ctx, sym_id, nlist.is_extern(), common));
    }
    plan.closed = true;
    plan.len = plan.fixed.len() + plan.syms.iter().map(|s| s.len()).sum::<usize>() + 1;
    plan
}

/// A symbol's debug notes, if it gets any.
fn symbol_stabs<E: Target>(
    ctx: &Context<E>,
    sym_id: crate::symbol::SymbolId,
    is_extern: bool,
    common: bool,
) -> Option<SymbolStabs> {
    let sym = &ctx.symbols[sym_id];
    let global = SymbolStabs { sym: sym_id, size: 0, n_sect: 0, n_type: N_GSYM };
    let Some(isec) = sym.input_section().map(|i| i as usize) else {
        // A -r output keeps a common undefined; it has no address.
        return common.then_some(global);
    };
    let isec = &ctx.isecs[ctx.resolve_isec(isec)];
    // ld-prime notes no exception tables' labels and no ivar offsets,
    // nor the method lists it rewrote in the relative form, atoms of
    // its own.
    let hdr = ctx.hdr_of(isec);
    let text = hdr.segname_is("__TEXT");
    if !isec.is_alive()
        || (text && (hdr.sectname_is("__gcc_except_tab") || hdr.sectname_is("__objc_methlist")))
        || (hdr.segname_is("__DATA") && hdr.sectname_is("__objc_ivar"))
    {
        return None;
    }
    let n_sect = ctx.isec_n_sect(isec);
    let is_text = text && hdr.flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0;
    Some(if is_text {
        SymbolStabs { size: isec.size, n_sect, n_type: N_FUN, ..global }
    } else if is_extern {
        global
    } else {
        SymbolStabs { n_sect, n_type: N_STSYM, ..global }
    })
}

/// The object whose stabs note each tentative definition that no real
/// one overrode: the first live object that declares it.
pub fn common_stab_owners<E: Target>(
    ctx: &Context<E>,
) -> hashbrown::HashMap<crate::symbol::SymbolId, usize> {
    let per_obj: Vec<Vec<crate::symbol::SymbolId>> = ctx
        .objs
        .par_iter()
        .map(|obj| {
            if !obj.is_alive || !obj.has_debug_info {
                return Vec::new();
            }
            let r = obj.global_range();
            obj.nlists[r.clone()]
                .iter()
                .zip(&obj.symbols[r])
                .filter(|&(nlist, &id)| {
                    let sym = &ctx.symbols[id];
                    !nlist.is_stab()
                        && nlist.is_common()
                        && (sym.is_common()
                            || matches!(sym.file(), Some(FileId::Obj(o)) if ctx.is_internal(o as usize)))
                })
                .map(|(_, &id)| id)
                .collect()
        })
        .collect();
    let mut owners = hashbrown::HashMap::new();
    for (obj_idx, ids) in per_obj.into_iter().enumerate() {
        for id in ids {
            owners.entry(id).or_insert(obj_idx);
        }
    }
    owners
}

/// An N_SO with an empty name: it closes an object's stabs, and
/// ld-prime opens the stabs of an image with one too.
const STAB_END: NList = NList { n_strx: 1, n_type: N_SO, n_sect: 1, n_desc: 0, n_value: 0 };

/// A final image's local symbols in ld-prime's order: the non-external
/// symbols it keeps, the private externals it demotes, the linker's own
/// names and the objc_msgSend$ stubs, all by address. Names at one
/// address are aliases of one atom, which ld-prime names by its
/// highest-ranked symbol - a strong external, then a private external,
/// a local, a weak definition, each rank by descending name - and it
/// lists the other names in that order before the atom's own (a strong
/// external's goes with the externals). -x keeps only private externals.
fn plan_local_symbols<E: Target>(
    ctx: &Context<E>,
    pexts: &[usize],
    sorted_globals: &[crate::symbol::SymbolId],
) -> Vec<LocalEnt> {
    type Ent = LocalEnt;
    const PEXT: u8 = 0;
    const LOCAL: u8 = 1;
    const WEAK: u8 = 2;
    let local =
        |n_sect: u8, n_value: u64| NList { n_strx: 0, n_type: N_SECT, n_sect, n_desc: 0, n_value };

    // -non_global_symbols_no_strip_list / _strip_list filter local
    // symbols by name; stabs unaffected.
    let is_listed_out = |name: &[u8]| {
        ctx.args.local_keep_list.as_ref().is_some_and(|keep| keep.find(name) == -1)
            || ctx.args.local_strip_list.find(name) != -1
    };

    let mut ents: Vec<Ent> = Vec::new();
    if !ctx.args.strip_locals {
        let per_obj: Vec<Vec<Ent>> = ctx
            .objs
            .par_iter()
            .map(|obj| {
                let mut out = Vec::new();
                if !obj.is_alive {
                    return out;
                }
                let aliases = objc_list_aliases(ctx, obj);
                for i in obj.local_range() {
                    let (nlist, sym_id) = (&obj.nlists[i], obj.symbols[i]);
                    let sym = &ctx.symbols[sym_id];
                    if nlist.is_stab()
                        || nlist.is_extern()
                        || !keep_local_symbol_in(
                            ctx,
                            sym.name(),
                            sym.input_section(),
                            nlist.n_type & N_PEXT != 0,
                            aliases.contains(&i),
                        )
                    {
                        continue;
                    }
                    if is_listed_out(sym.name().as_bytes()) {
                        continue;
                    }
                    let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
                    let isec = ctx.resolve_isec(isec);
                    if !matches!(sym.file(), Some(FileId::Obj(_))) || !ctx.isecs[isec].is_alive() {
                        continue;
                    }
                    let ent = local(ctx.isec_n_sect(&ctx.isecs[isec]), 0);
                    out.push((
                        ctx.sym_addr(sym_id),
                        LOCAL,
                        sym.name().as_bytes(),
                        ent,
                        Some(sym_id),
                    ));
                }
                out
            })
            .collect();
        ents = per_obj.concat();

        // Locals the linker named itself, on synthesized data whose
        // addresses are final by now.
        for &(name, isec) in &ctx.extra_local_syms {
            let sec = &ctx.isecs[isec as usize];
            if sec.is_alive() && sec.output_section().is_some() {
                let addr = ctx.isec_addr(isec as usize);
                ents.push((addr, LOCAL, name.as_bytes(), local(ctx.isec_n_sect(sec), addr), None));
            }
        }
        // The selector stubs, each a non-external symbol with N_PEXT
        // set (nm: "was a private external"), as ld64 lists them -
        // NetNewsWire's debug dylib has 851 _objc_msgSend$... entries.
        let hdr = &ctx.objc_stubs.hdr;
        for (i, &(sym, _)) in ctx.objc_stubs.symbols.iter().enumerate() {
            let addr = hdr.addr + i as u64 * E::OBJC_STUB_SIZE;
            let ent = NList { n_type: N_PEXT | N_SECT, ..local(hdr.n_sect, addr) };
            ents.push((addr, PEXT, ctx.symbols[sym].name().as_bytes(), ent, None));
        }
        // The lazy-load helpers - a call helper, like a selector stub,
        // with N_PEXT set - and slots.
        let hdr = &ctx.lazy_helpers.hdr;
        for (i, h) in ctx.lazy_helpers.helpers.iter().enumerate() {
            let addr = ctx.lazy_helper_addr(i);
            let (rank, n_type) = match h.kind {
                crate::chunks::lazy_helpers::LazyUse::Call => (PEXT, N_PEXT | N_SECT),
                _ => (LOCAL, N_SECT),
            };
            let ent = NList { n_type, ..local(hdr.n_sect, addr) };
            ents.push((addr, rank, h.name.as_bytes(), ent, None));
        }
        let hdr = &ctx.lazy_load_got.hdr;
        for (i, &(_, name)) in ctx.lazy_load_got.slots.iter().enumerate() {
            let addr = hdr.addr + i as u64 * 8;
            ents.push((addr, LOCAL, name.as_bytes(), local(hdr.n_sect, addr), None));
        }
        // The range-extension thunks' entries, named as ld-prime names
        // its branch islands.
        for (addr, n_sect, name) in crate::thunks::island_symbols(ctx) {
            if !is_listed_out(name) {
                ents.push((addr, LOCAL, name, local(n_sect, addr), None));
            }
        }
    }

    // Private external symbols resolve globally but appear as locals
    // (with N_PEXT still set) in the output.
    for &i in pexts {
        let sym = &ctx.symbols[i];
        let id = Some(i as crate::symbol::SymbolId);
        let (ent, id) = match (sym.file(), sym.input_section()) {
            (_, Some(isec)) => {
                let isec = &ctx.isecs[ctx.resolve_isec(isec as usize)];
                (NList { n_type: N_SECT | N_PEXT, ..local(ctx.isec_n_sect(isec), 0) }, id)
            }
            // A hidden __mh_execute_header (an export list that omits
            // it, or -no_exported_symbols) sits in the first section,
            // the mach header.
            (Some(FileId::Obj(o)), None) if ctx.is_internal(o as usize) => {
                (NList { n_type: N_SECT | N_PEXT, ..local(1, 0) }, id)
            }
            (_, None) => (NList { n_type: N_ABS | N_PEXT, ..local(0, sym.value) }, None),
        };
        // A demoted weak definition keeps N_WEAK_DEF. A method list
        // rewritten in the relative form is ld-prime's own atom, whose
        // name is a plain local: Swift's protocol method lists are weak
        // private externals.
        let (rank, ent) = if names_relative_method_list(ctx, i as u32) {
            (LOCAL, NList { n_type: N_SECT, ..ent })
        } else if sym.is_weak_def() {
            (WEAK, NList { n_desc: N_WEAK_DEF, ..ent })
        } else {
            (PEXT, ent)
        };
        ents.push((ctx.sym_addr(i as u32), rank, sym.name().as_bytes(), ent, id));
    }

    ents.par_sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(b.2.cmp(a.2)));

    // Put each atom's own name after its aliases, unless a strong
    // external names the atom. Few atoms have aliases, so those are
    // found first, and only their addresses are looked for among the
    // externals.
    let aliased: Vec<usize> = (0..ents.len().saturating_sub(1))
        .into_par_iter()
        .filter(|&i| ents[i].0 == ents[i + 1].0 && (i == 0 || ents[i - 1].0 != ents[i].0))
        .collect();
    if !aliased.is_empty() {
        let addrs: Vec<u64> = aliased.iter().map(|&i| ents[i].0).collect();
        let named: Vec<std::sync::atomic::AtomicBool> =
            addrs.iter().map(|_| std::sync::atomic::AtomicBool::new(false)).collect();
        sorted_globals.par_iter().for_each(|&i| {
            let sym = &ctx.symbols[i];
            if !sym.is_weak_def()
                && sym.input_section().is_some()
                && let Ok(k) = addrs.binary_search(&ctx.sym_addr(i))
            {
                named[k].store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        for (&i, named) in aliased.iter().zip(named) {
            let n = ents[i..].iter().take_while(|e| e.0 == ents[i].0).count();
            if !named.into_inner() {
                ents[i..i + n].rotate_left(1);
            }
        }
    }
    ents
}

/// A local symbol table entry as plan_local_symbols sorts it: address,
/// rank, name, entry, and the symbol whose address fills n_value.
type LocalEnt = (u64, u8, &'static [u8], NList, Option<crate::symbol::SymbolId>);

/// Appends an entry and its name for each item, made by `f` on all cores
/// straight into the arrays' spare capacity, which the caller reserved.
fn par_push_entries<T: Sync>(
    names: &mut Vec<&'static [u8]>,
    entries: &mut Vec<(NList, Option<crate::symbol::SymbolId>)>,
    items: &[T],
    f: impl Fn(&T) -> (&'static [u8], NList, Option<crate::symbol::SymbolId>) + Sync,
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

/// Builds the output symbol table contents: local symbols in input order,
/// then defined globals and undefined symbols, each sorted by name.
/// Symbol values are filled in when the table is copied out, after
/// addresses are assigned.
pub fn create_output_symtab<E: Target>(
    ctx: &Context<E>,
    sorted_globals: &[crate::symbol::SymbolId],
) -> SymtabSection {
    use std::sync::atomic::{AtomicU32, Ordering};
    let mut data = SymtabSection::new();

    let t = ctx.timer("symtab-classify");
    // An import is listed only while live code or data refers to it:
    // after -dead_strip, ld-prime drops the imports only stripped
    // functions used. A reference is a relocation from a live
    // subsection or a stub or GOT slot (unwind personalities, the
    // selector stubs' _objc_msgSend and dyld_stub_binder have slots).
    let live_ref: Vec<std::sync::atomic::AtomicBool> =
        (0..ctx.symbols.syms.len()).map(|_| std::sync::atomic::AtomicBool::new(false)).collect();
    {
        use std::sync::atomic::Ordering;
        ctx.isecs
            .par_iter()
            .filter(|isec| {
                isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT
            })
            .for_each(|isec| {
                for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
                    if let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel) {
                        live_ref[id as usize].store(true, Ordering::Relaxed);
                    }
                }
            });
        let slots = ctx
            .stubs
            .symbols
            .iter()
            .chain(&ctx.got.got_syms)
            .copied()
            .chain(ctx.objc_stubs.msgsend_sym)
            .chain(ctx.stub_helper.dyld_stub_binder);
        for id in slots {
            live_ref[id as usize].store(true, Ordering::Relaxed);
        }
        // The pointer fields of synthesized records (merged category
        // lists, the class registrations) refer to symbols too.
        for blob in &ctx.data_blobs {
            for field in &blob.fields {
                if let DataField::Ptr(ObjcRef::Sym(id, _)) = field {
                    live_ref[*id as usize].store(true, Ordering::Relaxed);
                }
            }
        }
        // -u names an import the program must keep whether or not
        // anything refers to it, and an -alias of an import re-exports
        // it by name (the N_INDR entry points at the import's).
        for name in &ctx.args.forced_undefined {
            if let Some(id) = ctx.symbols.get(name) {
                live_ref[id as usize].store(true, Ordering::Relaxed);
            }
        }
        for &(_, target) in &ctx.indirect_aliases {
            live_ref[target as usize].store(true, Ordering::Relaxed);
        }
    }

    // One parallel pass classifies the whole symbol table - private
    // externals (emitted among the locals), defined globals and
    // undefineds - instead of three full scans over millions of
    // slots.
    #[derive(Clone, Copy, PartialEq)]
    enum Class {
        No,
        Pext,
        Undef,
    }
    let classes: Vec<Class> = (0..ctx.symbols.syms.len())
        .into_par_iter()
        .map(|i| {
            let sym = &ctx.symbols[i];
            if matches!(sym.file(), Some(FileId::Dylib(_))) {
                return if live_ref[i].load(std::sync::atomic::Ordering::Relaxed) {
                    Class::Undef
                } else {
                    Class::No
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
                // is not emitted (ld-prime keeps clang's
                // __OBJC_LABEL_PROTOCOL_$_X, demoted, but not an
                // l_OBJC_LABEL_PROTOCOL_$_X), nor one that names an
                // entry of a list ld-prime names none of (see
                // objc_list_aliases).
                let listed = sym.input_section().is_some_and(|isec| {
                    is_unnamed_objc_list(ctx.hdr_of(&ctx.isecs[isec as usize]))
                });
                if !keep_local_symbol(sym.name()) || listed {
                    return Class::No;
                }
                return Class::Pext;
            }
            Class::No
        })
        .collect();
    drop(t);

    // Local symbols, then the debugger's notes: N_AST paths and stabs.
    let pexts: Vec<usize> =
        (0..classes.len()).into_par_iter().filter(|&i| classes[i] == Class::Pext).collect();
    let t = ctx.timer("symtab-locals");
    let locals = plan_local_symbols(ctx, &pexts, sorted_globals);
    drop(t);

    // Debug stabs. Mach-O binaries don't carry DWARF; instead, for each
    // object with debug info the symbol table gets stab entries telling
    // the debugger where the object file is (N_OSO) and where its
    // functions and globals ended up, and the debugger reads the DWARF
    // from the objects. Each object's run is independent.
    let t = ctx.timer("symtab-stabs");
    let planned: Vec<StabPlan> = if ctx.args.strip_debug {
        Vec::new()
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        let commons = common_stab_owners(ctx);
        ctx.objs
            .par_iter()
            .enumerate()
            .map(|(obj_idx, _)| plan_object_stabs(ctx, obj_idx, &cwd, &commons))
            .collect()
    };
    drop(t);

    let t = ctx.timer("symtab-entries");
    // Undefined (imported) symbols, sorted by name.
    let mut undefs: Vec<usize> =
        (0..classes.len()).into_par_iter().filter(|&i| classes[i] == Class::Undef).collect();
    undefs.par_sort_unstable_by_key(|&i| crate::util::name_sort_key(ctx.symbols[i].name()));

    // Every range's size is known now: the entries and their names are
    // allocated once, and each range is filled in parallel. The names
    // are the strings layout_strings lays out below. The debug notes
    // are not among them: copy_symtab writes them from their plans.
    let nstabs: usize = planned.iter().map(|plan| plan.len()).sum();
    let total = locals.len()
        + ctx.args.add_ast_paths.len()
        + usize::from(nstabs != 0)
        + sorted_globals.len()
        + undefs.len();
    let mut names: Vec<&'static [u8]> = Vec::with_capacity(total);
    data.entries.reserve_exact(total);

    par_push_entries(&mut names, &mut data.entries, &locals, |&(_, _, name, ent, sym)| {
        (name, ent, sym)
    });
    let nplain = data.entries.len();
    drop(locals);

    // Swift AST paths for the debugger (-add_ast_path), as N_AST stabs.
    for path in &ctx.args.add_ast_paths {
        names.push(leak_bytes(path_bytes(path).to_vec()));
        data.entries.push((NList { n_strx: 0, n_type: N_AST, ..Default::default() }, None));
    }

    // ld-prime opens the stabs with a closing N_SO of its own.
    if nstabs != 0 {
        names.push(b"");
        data.entries.push((STAB_END, None));
    }
    let stabs_start = data.entries.len();
    let nlocal = stabs_start + nstabs;
    data.nlocal = nlocal as u32;

    // Defined global symbols, sorted by name; the caller sorted them
    // once for this table and the export trie both.
    par_push_entries(&mut names, &mut data.entries, sorted_globals, |&i| {
        let sym = &ctx.symbols[i];
        let (n_type, n_sect, mut n_desc) = match (sym.file(), sym.input_section()) {
            (_, Some(isec)) => {
                (N_SECT | N_EXT, ctx.isec_n_sect(&ctx.isecs[ctx.resolve_isec(isec as usize)]), 0)
            }
            // A synthesized symbol with no section (__mh_execute_header)
            // sits in the first section: the mach header. Nothing slides
            // a -static image without -pie, and there it is absolute.
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
        let ent = NList { n_strx: 0, n_type, n_sect, n_desc, n_value: 0 };
        (sym.name().as_bytes(), ent, Some(i))
    });
    data.nextdef = sorted_globals.len() as u32;

    // The imports. The library ordinal lives in the high byte of n_desc.
    par_push_entries(&mut names, &mut data.entries, &undefs, |&i| {
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
        (sym.name().as_bytes(), ent, None)
    });
    data.nundef = undefs.len() as u32;
    debug_assert_eq!(data.entries.len(), total);
    drop(t);

    // The string table, in ld-prime's layout: the externals' names, the
    // locals', then each object's notes' in a block of its own. No note
    // is among the entries, so none shares a string there.
    let t = ctx.timer("symtab-strings");
    let strtab_end = crate::chunks::symtab::layout_strings(
        &mut data.entries,
        &mut names,
        stabs_start,
        (0, &[]),
        &[],
    );
    data.names = names;

    // Each symbol's index, for the indirect symbol table, and its string,
    // for the notes naming it - but for the first local's, whose notes
    // ld-prime gives a copy of their own.
    let nsyms = ctx.symbols.syms.len();
    let entry_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    let strx_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    let nglobals = sorted_globals.len();
    (0..nplain).into_par_iter().chain(stabs_start..stabs_start + nglobals).for_each(|i| {
        if let (ent, Some(id)) = data.entries[i] {
            let index = if i < stabs_start { i } else { i + nstabs };
            entry_of[id as usize].store(index as u32, Ordering::Relaxed);
            if index != 0 {
                strx_of[id as usize].store(ent.n_strx, Ordering::Relaxed);
            }
        }
    });
    undefs.par_iter().enumerate().for_each(|(k, &id)| {
        entry_of[id].store((nlocal + nglobals + k) as u32, Ordering::Relaxed);
    });
    data.output_sym_indices = entry_of.into_iter().map(AtomicU32::into_inner).collect();
    data.strx_of = strx_of.into_iter().map(AtomicU32::into_inner).collect();

    // The notes' strings, each object's after the previous one's.
    let sizes: Vec<usize> =
        planned.par_iter().map(|plan| plan.strtab_size(ctx, &data.strx_of)).collect();
    let mut strx = strtab_end;
    data.stab_strx = Vec::with_capacity(planned.len() + 1);
    for size in sizes {
        data.stab_strx.push(strx as u32);
        strx += size;
    }
    data.stab_strx.push(strx as u32);
    data.strtab_size = strx.next_multiple_of(8);
    data.stabs = planned;
    data.stabs_start = stabs_start;
    data.nstabs = nstabs;

    // An alias of an imported symbol is an N_INDR entry whose n_value
    // is the string-table offset of the name it stands for; that
    // name is in the table already as the import's own entry. The
    // slot is detached from the symbol so copy_symtab leaves n_value
    // alone.
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
        let strx = data.entries[entry(t)].0.n_strx;
        let ent = &mut data.entries[entry(a)];
        ent.0.n_type = N_INDR | N_EXT;
        ent.0.n_sect = 0;
        ent.0.n_desc = 0;
        ent.0.n_value = strx as u64;
        ent.1 = None;
    }

    drop(t);

    data
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

/// Each symbol's entry among a symbol table's plain locals, [0, nplain),
/// and externals, [nlocal, len) - never a debug note - or u32::MAX.
pub fn symbol_entries(
    entries: &[(NList, Option<SymbolId>)],
    nplain: usize,
    nlocal: usize,
    nsyms: usize,
) -> Vec<AtomicU32> {
    let entry_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    (0..nplain).into_par_iter().chain(nlocal..entries.len()).for_each(|i| {
        if let Some(id) = entries[i].1 {
            entry_of[id as usize].store(i as u32, Ordering::Relaxed);
        }
    });
    entry_of
}

/// Lays out a symbol table's strings as ld-prime does and sets every
/// entry's n_strx. The strings follow the defined and undefined
/// externals, then the local symbols, then the debug notes, each entry
/// with a copy of its own - two locals of one name get two - except
/// that a note naming a symbol shares that symbol's string (though not
/// the first local's, which ld-prime copies again) and an empty name is
/// offset 1, after the table's leading " ". `stabs` gives where the
/// notes start and the symbol each one names, `entry_of` each symbol's
/// entry (symbol_entries), and [nlocal, len) are the externals. A note
/// that shares a string gets an empty name, so that afterwards an
/// entry's name is exactly the string to write at its n_strx. Returns
/// where the strings end.
pub fn layout_strings(
    entries: &mut [(NList, Option<SymbolId>)],
    names: &mut [&[u8]],
    nlocal: usize,
    stabs: (usize, &[Option<SymbolId>]),
    entry_of: &[AtomicU32],
) -> usize {
    // The notes that share a string, and the entries they share it
    // with: a plain local's or an external's, never another note's.
    let (stabs_start, names_of) = stabs;
    let shared: Vec<u32> = names[stabs_start..stabs_start + names_of.len()]
        .par_iter_mut()
        .zip(names_of)
        .map(|(name, id)| {
            let e = id.map_or(u32::MAX, |id| entry_of[id as usize].load(Ordering::Relaxed));
            if e == u32::MAX || e == 0 {
                return u32::MAX;
            }
            *name = b"";
            e
        })
        .collect();

    // The externals' strings come first, then the locals' and notes',
    // each block of entries at its prefix-summed offset.
    const CHUNK: usize = 1 << 16;
    let (locals, externs) = entries.split_at_mut(nlocal);
    let (local_names, extern_names) = names.split_at(nlocal);
    let chunks = || extern_names.par_chunks(CHUNK).chain(local_names.par_chunks(CHUNK));
    let size = |name: &&[u8]| if name.is_empty() { 0 } else { name.len() as u32 + 1 };
    let sums: Vec<u32> = chunks().map(|c| c.iter().map(size).sum()).collect();
    let mut bases = Vec::with_capacity(sums.len());
    let mut total = 2u32;
    for sum in sums {
        bases.push(total);
        total += sum;
    }
    externs
        .par_chunks_mut(CHUNK)
        .chain(locals.par_chunks_mut(CHUNK))
        .zip(chunks())
        .zip(bases)
        .for_each(|((ents, names), mut off)| {
            for ((ent, _), name) in ents.iter_mut().zip(names) {
                ent.n_strx = if name.is_empty() { 1 } else { off };
                off += size(name);
            }
        });

    // The notes that share take their entry's offset.
    let (head, rest) = entries.split_at_mut(stabs_start);
    let (notes, tail) = rest.split_at_mut(shared.len());
    let tail_start = stabs_start + shared.len();
    notes.par_iter_mut().zip(shared).for_each(|((ent, _), e)| {
        let e = e as usize;
        if e != u32::MAX as usize {
            let owner = if e < stabs_start { &head[e] } else { &tail[e - tail_start] };
            ent.n_strx = owner.0.n_strx;
        }
    });
    total as usize
}
