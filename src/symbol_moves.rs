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

use crate::chunks::symtab::keep_local_symbol;
use crate::cmdline::SymbolMove;
use crate::context::Context;
use crate::error::raw;
use crate::input_files::FileId;
use crate::macho::*;
use crate::output_sections::{canonical_section_flags, common_owners};
use crate::passes::resolved_file_name;
use crate::symbol::SymbolId;
use crate::target::Target;
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

/// The subsection a symbol names to ld-prime (an absolute symbol is one
/// of its own there): the subsection to move, the segment it is in,
/// what it holds and what ld-prime calls that in its warnings.
struct Subsec<'a> {
    /// The subsection the link kept (see Context::resolve_isec) - a
    /// record the linker rewrote in place of the input's too (see
    /// rewritten_records); none for an absolute symbol, or for a
    /// subsection the linker places itself (an input class reference
    /// folded into the GOT).
    isec: Option<u32>,
    /// Where ld-prime comes to the subsection (see Place).
    place: Place,
    segment: &'a [u8],
    content: Content,
    kind: &'static str,
}

/// Where ld-prime comes to a subsection in its walk over the files'
/// subsections, which orders its warnings: file by file, an object's
/// subsections of its sections, then of its common symbols (mold makes
/// their subsections after every input's), then of its absolute
/// symbols; after the objects, the files ld-prime makes itself, each
/// with its subsections in the order made - the -alias names of the
/// command line's (in the options' order), the Objective-C one's
/// relative method lists, then the thread-local variables'
/// descriptors. (A file and a subsection, or an -alias.)
type Place = (u32, u64);

const ALIASES_FILE: u32 = u32::MAX - 2;
const OBJC_FILE: u32 = u32::MAX - 1;
const TLV_FILE: u32 = u32::MAX;

/// The file of a symbol that names a subsection (see
/// for_each_subsec_symbol): an input object, or for an -alias name
/// ld-prime's command-line-aliases-file, where the name stands for the
/// subsection of its base, which it gives as well.
#[derive(Clone, Copy)]
enum SymbolFile {
    Obj(usize),
    Aliases(SymbolId),
}

/// What a subsection of an input section holds, and ld-prime's name for
/// it.
fn content_of(seg: &[u8], sect: &[u8], flags: u32) -> (Content, &'static str) {
    use Content::*;
    let flags = canonical_section_flags(seg, sect, flags);
    if flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
        let kind =
            if (seg, sect) == (b"__TEXT", b"__StaticInit") { "staticInit" } else { "function" };
        return (Code, kind);
    }
    match flags & SECTION_TYPE {
        S_ZEROFILL | S_GB_ZEROFILL => return (Data, "bss"),
        S_THREAD_LOCAL_REGULAR => return (Data, "thread-data"),
        S_THREAD_LOCAL_ZEROFILL => return (Data, "thread-bss"),
        S_THREAD_LOCAL_VARIABLES => return (Data, "thread-vars"),
        S_4BYTE_LITERALS => return (Code, "literal4"),
        S_8BYTE_LITERALS => return (Code, "literal8"),
        S_16BYTE_LITERALS => return (Code, "literal16"),
        S_CSTRING_LITERALS => {
            let kind = match (seg, sect) {
                (b"__TEXT", b"__objc_methname") => "objc-method-name",
                (b"__TEXT", b"__objc_classname") => "objc-class-name",
                (b"__TEXT", b"__objc_methtype") => "objc-method-type",
                (b"__TEXT", b"__oslogstring") => "os-log-strings",
                _ => "c-string-literal",
            };
            return (Code, kind);
        }
        _ => {}
    }
    match (seg, sect) {
        (b"__TEXT", b"__gcc_except_tab") => (Code, "LSDA"),
        (b"__DATA", b"__data") => (Data, "data"),
        (b"__DATA", b"__cfstring") => (Data, "cfstring"),
        (b"__DATA", b"__auth_ptr") => (Data, "auth-ptr"),
        (b"__DATA", b"__objc_data") => (Data, "objc-data"),
        (b"__DATA", b"__objc_const") => (Data, "objc-const"),
        (b"__DATA", b"__objc_ivar") => (Data, "objc-ivar"),
        (b"__DATA", b"__objc_superrefs") => (Data, "objc-super-ref"),
        (b"__DATA", b"__objc_protolist") => (Data, "objc-protocol-list"),
        (b"__DATA", b"__objc_protorefs") => (Data, "objc-protocol-ref"),
        _ => (Other, "custom"),
    }
}

/// Decides which subsections the lists move to which segments, by
/// subsection, and warns about each symbol a list names (not one a
/// pattern matches) that its option cannot move, as ld-prime does: of
/// -move_to_ro_segment's first, then of -move_to_rw_segment's, each in
/// the order of the subsections (see Place). The two options look at a
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
    let (mut rw_warnings, mut ro_warnings) = (Vec::new(), Vec::new());
    let warning = |file: SymbolFile, id: SymbolId, subsec: &Subsec, list: &SymbolMove| {
        let place = (subsec.place, ctx.symbols[id].value);
        // ld-prime makes the thread-local variables' descriptors, the
        // relative method lists and the -alias names itself.
        let file = match (subsec.kind, file) {
            ("thread-vars", _) => "tlv-file".into(),
            ("objc-method-list", _) => "objc-file".into(),
            (_, SymbolFile::Aliases(_)) => "command-line-aliases-file".into(),
            (_, SymbolFile::Obj(obj)) => resolved_file_name(ctx.objs[obj].mf),
        };
        let what = if subsec.content == Content::Code { "code" } else { "not code" };
        let msg = crate::error::render(format_args!(
            "cannot move symbol '{}' ({file}) to segment '{}' because symbol is {what} (is {})",
            raw(ctx.symbols[id].name()),
            raw(&list.segment),
            subsec.kind
        ));
        (place, msg)
    };

    for_each_subsec_symbol(ctx, |file, id, subsec| {
        // A list entry file:name names the symbol of an object of that
        // leaf name. An -alias name stands for its base's subsection,
        // which a list names by either name.
        let (leaf, base) = match file {
            SymbolFile::Obj(obj) => {
                (ctx.objs[obj].mf.name.file_name().map_or(&[][..], |f| f.as_bytes()), None)
            }
            SymbolFile::Aliases(base) => (&b"command-line-aliases-file"[..], Some(base)),
        };
        let base_name = base.map(|base| ctx.symbols[base].name());
        let names = [Some(ctx.symbols[id].name()), base_name];
        let find = |lists: &[SymbolMove]| {
            lists.iter().enumerate().find_map(|(i, list)| {
                let found = names.iter().flatten().map(|&name| match list.symbols.find(name) {
                    -1 => list.symbols.find(&[leaf, b":", name].concat()),
                    found => found,
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
                rw_warnings.push(warning(file, id, &subsec, &args.move_to_rw[i]));
            }
        }
        if let Some((i, named)) = find(&args.move_to_ro)
            && ro_segs[i] != subsec.segment
        {
            if subsec.content != Content::Data {
                chosen = chosen.or(Some(Move { option: MoveOption::Ro, segment: ro_segs[i] }));
            } else if named {
                ro_warnings.push(warning(file, id, &subsec, &args.move_to_ro[i]));
            }
        }
        if chosen.is_none()
            && find(&args.dirty_data).is_some()
            && !matches!(subsec.kind, "thread-data" | "thread-bss")
        {
            chosen = Some(Move { option: MoveOption::Dirty, segment: b"__DATA_DIRTY" });
        }
        if let (Some(m), Some(isec)) = (chosen, subsec.isec) {
            moves.entry(isec).or_insert(m);
        }
    });

    ro_warnings.sort_by_key(|&(place, _)| place);
    rw_warnings.sort_by_key(|&(place, _)| place);
    for (_, msg) in ro_warnings.iter().chain(&rw_warnings) {
        crate::warn!("{}", raw(msg));
    }
    moves
}

/// Calls `f` with each symbol that names a live subsection to ld-prime,
/// with its file (see subsec_named): the objects' symbols, then the
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
        for (nlist, &id) in obj.nlists.iter().zip(&obj.symbols) {
            if let Some(subsec) = subsec_named(ctx, &commons, &rewritten, i, nlist, id) {
                f(SymbolFile::Obj(i), id, subsec);
            }
        }
    }

    for (k, (base, alias)) in object_aliases(ctx).enumerate() {
        let obj = match ctx.symbols[base].file() {
            Some(FileId::Obj(obj)) => obj as usize,
            _ => continue,
        };
        let Some(i) = ctx.objs[obj].symbols.iter().position(|&id| id == base) else {
            continue;
        };
        let nlist = &ctx.objs[obj].nlists[i];
        if let Some(subsec) = subsec_named(ctx, &commons, &rewritten, obj, nlist, base) {
            let subsec = Subsec { place: (ALIASES_FILE, k as u64), ..subsec };
            f(SymbolFile::Aliases(base), alias, subsec);
        }
    }
}

/// The -alias names of definitions in objects, in the options' order,
/// each with its base: ld-prime makes each name a subsection of its
/// command-line-aliases-file, which stands for its base's (see
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
/// the input's symbols name and a symbol move takes along as ld-prime
/// does its own: the Objective-C data category merging rebuilt (see
/// objc::merge_objc_categories), false, and the method lists rewritten
/// in the relative form (see objc::convert_objc_method_lists), true.
fn rewritten_records<E: Target>(ctx: &Context<E>) -> hashbrown::HashMap<u32, bool> {
    let blobs = ctx.data_blobs.iter().map(|b| (b.isec, false));
    let lists = ctx.objc_methlist.lists.iter().map(|l| (l.isec, true));
    blobs.chain(lists).collect()
}

/// The subsection ld-prime names by the symbol `id` of object `obj`,
/// whose entry is `nlist`: a definition the link kept, external or
/// local but no assembler label (see symtab::keep_local_symbol), in
/// a section or absolute; or a common symbol, if the object's tentative
/// definition is the one its subsection stands for (see common_owners).
/// A method list ld-prime rewrote in the relative form (see
/// rewritten_records) is code to it, which its own objc-file holds.
fn subsec_named<'a, E: Target>(
    ctx: &'a Context<E>,
    commons: &hashbrown::HashMap<u32, u32>,
    rewritten: &hashbrown::HashMap<u32, bool>,
    obj: usize,
    nlist: &NList,
    id: SymbolId,
) -> Option<Subsec<'a>> {
    let sym = &ctx.symbols[id];
    if nlist.is_common() {
        let isec = sym.input_section().filter(|isec| commons.get(isec) == Some(&(obj as u32)))?;
        let (content, kind) = (Content::Data, "common");
        let place = (obj as u32, isec as u64);
        return Some(Subsec { isec: Some(isec), place, segment: b"__DATA", content, kind });
    }
    if nlist.is_stab()
        || !matches!(nlist.n_type(), N_SECT | N_ABS)
        || sym.file() != Some(FileId::Obj(obj as u32))
    {
        return None;
    }
    let isec = sym.input_section();
    if !nlist.is_extern() && !keep_local_symbol(sym.name()) {
        return None;
    }
    let Some(isec) = isec else {
        let (content, kind) = (Content::Data, "data");
        let place = (obj as u32, u64::MAX);
        return Some(Subsec { isec: None, place, segment: b"", content, kind });
    };
    let kept = ctx.resolve_isec(isec as usize);
    if !ctx.isecs[kept].is_alive() {
        return None;
    }
    let hdr = ctx.hdr_of(&ctx.isecs[isec as usize]);
    let rewritten = rewritten.get(&(kept as u32));
    let (content, kind) = match rewritten {
        Some(true) => (Content::Code, "objc-method-list"),
        _ => content_of(hdr.segname(), hdr.sectname(), hdr.flags),
    };
    let movable = rewritten.is_some() || !ctx.isecs[kept].is_placed();
    let file = match kind {
        "objc-method-list" => OBJC_FILE,
        "thread-vars" => TLV_FILE,
        _ => obj as u32,
    };
    Some(Subsec {
        isec: movable.then_some(kept as u32),
        place: (file, isec as u64),
        segment: hdr.segname(),
        content,
        kind,
    })
}
