//! -move_to_rw_segment, -move_to_ro_segment and -dirty_data_list: lists
//! of symbols whose subsections move to another segment - data written
//! at run time (rw) or code and constants (ro), to put them on pages of
//! their own, and the data a shared-cache dylib dirties, to
//! __DATA_DIRTY. Each option takes the first of its lists that names one
//! of a subsection's symbols, or matches it with a pattern, and moves
//! the subsection to the section of its name in the list's segment (see
//! output_sections::assign_input_sections). A new segment follows the
//! linker's own, read-write as any other ld-prime doesn't know - but one
//! made for moved code, which mold makes executable (see
//! chunks::segment_prots). Fixups, symbols and -order_file treat a moved
//! subsection as any other.

use std::os::unix::ffi::OsStrExt;

use crate::arch::Target;
use crate::chunks::symtab::keep_local_symbol;
use crate::cmdline::SymbolMove;
use crate::context::Context;
use crate::error::{RawPath, raw};
use crate::input_files::{FileId, canonical_section_flags};
use crate::macho::*;
use crate::output_sections::common_owners;
use crate::symbol::SymbolId;
use crate::util::leak_bytes;

/// The option that moves a subsection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveOption {
    Rw,
    Ro,
    Dirty,
}

/// Where a symbol move sends a subsection.
#[derive(Clone, Copy)]
pub struct Move {
    pub option: MoveOption,
    /// The segment, cut to 16 bytes.
    pub segment: &'static [u8],
}

/// What a subsection holds, as ld-prime tells which option may move it:
/// code, with the constants the compiler makes along with it in __TEXT
/// (strings, literals and exception tables), that -move_to_rw_segment
/// does not move, data written at run time (by the program, or the
/// Objective-C runtime) that -move_to_ro_segment does not, and anything
/// else, other constants and sections of other names, that either does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Content {
    Code,
    Data,
    Other,
}

/// The subsection a symbol names (an absolute symbol is one of its
/// own): the subsection to move, the segment it is in and what it holds.
struct Subsec<'a> {
    /// The subsection the link kept (see Context::resolve_isec) - a
    /// record the linker rewrote in place of the input's too (see
    /// rewritten_records); none for an absolute symbol, or for a
    /// subsection the linker places itself (an input class reference
    /// folded into the GOT).
    isec: Option<u32>,
    segment: &'a [u8],
    content: Content,
    /// Thread-local data, the template each thread's copy starts from,
    /// which -dirty_data_list leaves in place.
    tlv_template: bool,
}

/// The file of a symbol that names a subsection (see
/// for_each_subsec_symbol): an input object, or for an -alias name the
/// command line, where the name stands for the subsection of its base,
/// which it gives as well.
#[derive(Clone, Copy)]
enum SymbolFile {
    Obj(usize),
    Aliases(SymbolId),
}

/// What a subsection of an input section holds, by the section's
/// canonical flags (see canonical_section_flags).
fn content_of(seg: &[u8], sect: &[u8], flags: u32) -> Content {
    use Content::*;
    if flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
        return Code;
    }
    match flags & SECTION_TYPE {
        S_ZEROFILL | S_GB_ZEROFILL => return Data,
        S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL | S_THREAD_LOCAL_VARIABLES => return Data,
        S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS | S_CSTRING_LITERALS => {
            return Code;
        }
        _ => {}
    }
    match (seg, sect) {
        (b"__TEXT", b"__gcc_except_tab") => Code,
        (
            b"__DATA",
            b"__data" | b"__cfstring" | b"__auth_ptr" | b"__objc_data" | b"__objc_const"
            | b"__objc_ivar" | b"__objc_superrefs" | b"__objc_protolist" | b"__objc_protorefs",
        ) => Data,
        _ => Other,
    }
}

/// Decides which subsections the lists move to which segments, by
/// subsection, and warns about each symbol a list names (not one a
/// pattern matches) that its option cannot move. The two options look at a
/// subsection apart, each with the first of its lists that names it:
/// one whose segment is the subsection's own leaves it in place
/// silently, -move_to_rw_segment's wins over the other's, and
/// -dirty_data_list's applies only if neither moves the subsection, nor
/// to the thread-local template (and only in __DATA, see
/// output_sections::SectionMap::moved_name). A -r link moves nothing
/// (cmdline::check_relocatable).
pub(crate) fn find_moves<E: Target>(ctx: &Context<E>) -> hashbrown::HashMap<u32, Move> {
    let args = &ctx.args;
    let mut moves = hashbrown::HashMap::new();
    if args.relocatable
        || (args.move_to_rw.is_empty() && args.move_to_ro.is_empty() && args.dirty_data.is_empty())
    {
        return moves;
    }
    let segments = |lists: &[SymbolMove]| -> Vec<&'static [u8]> {
        lists.iter().map(|list| leak_bytes(cut_name(&list.segment).to_vec())).collect()
    };
    let (rw_segs, ro_segs) = (segments(&args.move_to_rw), segments(&args.move_to_ro));
    let warn = |file: SymbolFile, id: SymbolId, subsec: &Subsec, list: &SymbolMove| {
        let file = match file {
            SymbolFile::Obj(obj) => ctx.objs[obj].mf.name.raw(),
            SymbolFile::Aliases(_) => raw(b"-alias"),
        };
        let what = if subsec.content == Content::Code { "code" } else { "data" };
        crate::warn!(
            "cannot move symbol '{}' ({file}) to segment '{}' because it is {what}",
            raw(ctx.symbols[id].name()),
            raw(&list.segment),
        );
    };

    for_each_subsec_symbol(ctx, |file, id, subsec| {
        // A list entry file:name names the symbol of an object of that
        // leaf name. An -alias name stands for its base's subsection,
        // which a list names by either name.
        let (leaf, base) = match file {
            SymbolFile::Obj(obj) => (ctx.objs[obj].mf.name.file_name(), None),
            SymbolFile::Aliases(base) => (None, Some(base)),
        };
        let base_name = base.map(|base| ctx.symbols[base].name());
        let names = [Some(ctx.symbols[id].name()), base_name];
        let find = |lists: &[SymbolMove]| {
            lists.iter().enumerate().find_map(|(i, list)| {
                let found =
                    names.iter().flatten().map(|&name| match (list.symbols.find(name), leaf) {
                        (-1, Some(leaf)) => {
                            list.symbols.find(&[leaf.as_bytes(), b":", name].concat())
                        }
                        (found, _) => found,
                    });
                let found = found.max().unwrap();
                (found >= 0).then_some((i, found == 1))
            })
        };

        let mut chosen = None;
        if let Some((i, named)) = find(&args.move_to_rw)
            && rw_segs[i] != subsec.segment
        {
            if subsec.content != Content::Code {
                chosen = Some(Move { option: MoveOption::Rw, segment: rw_segs[i] });
            } else if named {
                warn(file, id, &subsec, &args.move_to_rw[i]);
            }
        }
        if let Some((i, named)) = find(&args.move_to_ro)
            && ro_segs[i] != subsec.segment
        {
            if subsec.content != Content::Data {
                chosen = chosen.or(Some(Move { option: MoveOption::Ro, segment: ro_segs[i] }));
            } else if named {
                warn(file, id, &subsec, &args.move_to_ro[i]);
            }
        }
        if chosen.is_none() && find(&args.dirty_data).is_some() && !subsec.tlv_template {
            chosen = Some(Move { option: MoveOption::Dirty, segment: b"__DATA_DIRTY" });
        }
        if let (Some(m), Some(isec)) = (chosen, subsec.isec) {
            moves.entry(isec).or_insert(m);
        }
    });
    moves
}

/// Calls `f` with each symbol that names a live subsection, with its
/// file (see subsec_named): the objects' symbols, then the
/// -alias names of a definition in one, each with its base's
/// subsection.
fn for_each_subsec_symbol<'a, E: Target>(
    ctx: &'a Context<E>,
    mut f: impl FnMut(SymbolFile, SymbolId, Subsec<'a>),
) {
    let commons = common_owners(ctx);
    let rewritten = rewritten_records(ctx);
    for (i, obj) in ctx.objs.iter().enumerate() {
        if !obj.is_alive || ctx.is_internal(i) {
            continue;
        }
        for (msym, &id) in obj.mach_syms.iter().zip(&obj.symbols) {
            if let Some(subsec) = subsec_named(ctx, &commons, &rewritten, i, msym, id) {
                f(SymbolFile::Obj(i), id, subsec);
            }
        }
    }

    for (base, alias) in object_aliases(ctx) {
        let obj = match ctx.symbols[base].file() {
            Some(FileId::Obj(obj)) => obj as usize,
            _ => continue,
        };
        let Some(i) = ctx.objs[obj].symbols.iter().position(|&id| id == base) else {
            continue;
        };
        let msym = &ctx.objs[obj].mach_syms[i];
        if let Some(subsec) = subsec_named(ctx, &commons, &rewritten, obj, msym, base) {
            f(SymbolFile::Aliases(base), alias, subsec);
        }
    }
}

/// The -alias names of definitions in objects, in the options' order,
/// each with its base, whose subsection the name stands for (see
/// passes::add_synthetic_symbols; a dylib's base leaves an indirect
/// symbol, and a name an input defines stays the input's).
pub(crate) fn object_aliases<E: Target>(
    ctx: &Context<E>,
) -> impl Iterator<Item = (SymbolId, SymbolId)> + '_ {
    ctx.args.aliases.iter().filter_map(|(base, alias)| {
        let (base, alias) = (ctx.symbols.get(base)?, ctx.symbols.get(alias)?);
        let (b, a) = (&ctx.symbols[base], &ctx.symbols[alias]);
        let defined = matches!(b.file(), Some(FileId::Obj(_)))
            && (a.file(), a.input_section(), a.value) == (b.file(), b.input_section(), b.value);
        defined.then_some((base, alias))
    })
}

/// The records the linker rewrote in place of input subsections, which
/// the input's symbols name and a symbol move takes along: the
/// Objective-C data category merging rebuilt (see
/// objc::merge_objc_categories), false, and the method lists rewritten
/// in the relative form (see objc::convert_objc_method_lists), true.
fn rewritten_records<E: Target>(ctx: &Context<E>) -> hashbrown::HashMap<u32, bool> {
    let blobs = ctx.data_blobs.iter().map(|b| (b.isec, false));
    let lists = ctx.objc_methlist.lists.iter().map(|l| (l.isec, true));
    blobs.chain(lists).collect()
}

/// The subsection the symbol `id` of object `obj`, whose entry is
/// `msym`, names: a definition the link kept, external or local but no
/// assembler label (see symtab::keep_local_symbol), in a section or
/// absolute; or a common symbol, if the object's tentative definition is
/// the one its subsection stands for (see common_owners). A method list
/// rewritten in the relative form (see rewritten_records) is code, as
/// ld-prime counts it.
fn subsec_named<'a, E: Target>(
    ctx: &'a Context<E>,
    commons: &hashbrown::HashMap<u32, u32>,
    rewritten: &hashbrown::HashMap<u32, bool>,
    obj: usize,
    msym: &MachSym,
    id: SymbolId,
) -> Option<Subsec<'a>> {
    let sym = &ctx.symbols[id];
    if msym.is_common() {
        let isec = sym.input_section().filter(|isec| commons.get(isec) == Some(&(obj as u32)))?;
        let content = Content::Data;
        return Some(Subsec { isec: Some(isec), segment: b"__DATA", content, tlv_template: false });
    }
    if msym.is_stab()
        || !matches!(msym.ty(), N_SECT | N_ABS)
        || sym.file() != Some(FileId::Obj(obj as u32))
        || (!msym.is_extern() && !keep_local_symbol(sym.name()))
    {
        return None;
    }
    // An absolute symbol is a subsection of its own, in no segment.
    let Some(isec) = sym.input_section() else {
        let content = Content::Data;
        return Some(Subsec { isec: None, segment: b"", content, tlv_template: false });
    };
    let kept = ctx.resolve_isec(isec as usize);
    if !ctx.isecs[kept].is_alive() {
        return None;
    }
    let hdr = ctx.hdr_of(&ctx.isecs[isec as usize]);
    let flags = canonical_section_flags(hdr.segname(), hdr.sectname(), hdr.flags);
    let rewritten = rewritten.get(&(kept as u32));
    let content = match rewritten {
        Some(true) => Content::Code,
        _ => content_of(hdr.segname(), hdr.sectname(), flags),
    };
    let tlv_template =
        matches!(flags & SECTION_TYPE, S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL);
    let movable = rewritten.is_some() || !ctx.isecs[kept].is_placed();
    Some(Subsec {
        isec: movable.then_some(kept as u32),
        segment: hdr.segname(),
        content,
        tlv_template,
    })
}
