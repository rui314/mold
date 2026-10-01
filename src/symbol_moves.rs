//! -move_to_rw_segment, -move_to_ro_segment and -dirty_data_list: lists
//! of symbols whose atoms move to another segment - data written at run
//! time (rw) or code and constants (ro), to put them on pages of their
//! own, and the data a shared-cache dylib dirties, to __DATA_DIRTY.
//! Each option takes the first of its lists that names one of an atom's
//! symbols, or matches it with a pattern, and moves the atom to the
//! section of its name in the list's segment (see
//! output_sections::assign_input_sections). A new segment follows the
//! linker's own, read-write as any other ld-prime doesn't know - but one
//! made for moved code, which mold makes executable (see
//! chunks::segment_prots). Fixups, symbols and -order_file treat a moved
//! atom as any other.

use std::os::unix::ffi::OsStrExt;

use crate::chunks::symtab::keep_local_symbol_in;
use crate::cmdline::SymbolMove;
use crate::context::Context;
use crate::input_files::FileId;
use crate::macho::*;
use crate::output_sections::{canonical_section_flags, common_owners};
use crate::passes::resolved_file_name;
use crate::symbol::SymbolId;
use crate::target::Target;

/// The option that moves an atom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveOption {
    Rw,
    Ro,
    Dirty,
}

impl MoveOption {
    pub(crate) fn name(self) -> &'static str {
        match self {
            MoveOption::Rw => "-move_to_rw_segment",
            MoveOption::Ro => "-move_to_ro_segment",
            MoveOption::Dirty => "-dirty_data_list",
        }
    }
}

/// Where a symbol move sends a subsection.
#[derive(Clone, Copy)]
pub struct Move {
    pub option: MoveOption,
    /// The segment, cut to 16 bytes.
    pub segment: &'static str,
}

/// What an atom holds, as ld-prime tells which option may move it: code,
/// with the constants the compiler makes along with it in __TEXT
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

/// The atom a symbol names: the subsection to move, the segment it is
/// in, what it holds and what ld-prime calls that in its warnings.
struct Atom<'a> {
    /// The subsection the link kept of the atom (see
    /// Context::resolve_isec) - a record the linker rewrote in place of
    /// the input's too (see rewritten_records); none for an absolute
    /// symbol, or for another subsection the linker places itself (an
    /// input class reference folded into the GOT).
    isec: Option<u32>,
    /// Where ld-prime comes to the atom (see Place).
    place: Place,
    segment: &'a str,
    content: Content,
    kind: &'static str,
}

/// Where ld-prime comes to an atom in its walk over the files' atoms,
/// which orders its warnings and its -trace_symbol_layout lines: file
/// by file, an object's atoms of its sections, then of its common
/// symbols (mold makes their subsections after every input's), then of
/// its absolute symbols; after the objects, the files ld-prime makes
/// itself, each with its atoms in the order made - the Objective-C
/// one's relative method lists, then the thread-local variables'
/// descriptors. (A file and a subsection.)
type Place = (u32, u64);

const OBJC_FILE: u32 = u32::MAX - 1;
const TLV_FILE: u32 = u32::MAX;

/// What an atom of an input section holds, and ld-prime's name for it.
fn content_of(seg: &str, sect: &str, flags: u32) -> (Content, &'static str) {
    use Content::*;
    let flags = canonical_section_flags(seg, sect, flags);
    if flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
        let kind =
            if (seg, sect) == ("__TEXT", "__StaticInit") { "staticInit" } else { "function" };
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
                ("__TEXT", "__objc_methname") => "objc-method-name",
                ("__TEXT", "__objc_classname") => "objc-class-name",
                ("__TEXT", "__objc_methtype") => "objc-method-type",
                ("__TEXT", "__oslogstring") => "os-log-strings",
                _ => "c-string-literal",
            };
            return (Code, kind);
        }
        _ => {}
    }
    match (seg, sect) {
        ("__TEXT", "__gcc_except_tab") => (Code, "LSDA"),
        ("__DATA", "__data") => (Data, "data"),
        ("__DATA", "__cfstring") => (Data, "cfstring"),
        ("__DATA", "__auth_ptr") => (Data, "auth-ptr"),
        ("__DATA", "__objc_data") => (Data, "objc-data"),
        ("__DATA", "__objc_const") => (Data, "objc-const"),
        ("__DATA", "__objc_ivar") => (Data, "objc-ivar"),
        ("__DATA", "__objc_superrefs") => (Data, "objc-super-ref"),
        ("__DATA", "__objc_protolist") => (Data, "objc-protocol-list"),
        ("__DATA", "__objc_protorefs") => (Data, "objc-protocol-ref"),
        _ => (Other, "custom"),
    }
}

/// Decides which subsections the lists move to which segments, by
/// subsection, and warns about each symbol a list names (not one a
/// pattern matches) that its option cannot move, as ld-prime does: of
/// -move_to_ro_segment's first, then of -move_to_rw_segment's, each in
/// the order of the atoms (see Place). The two
/// options look at an atom apart, each with the first of its lists
/// that names it: one whose segment is the atom's own leaves it in
/// place silently, -move_to_rw_segment's wins over the other's, and
/// -dirty_data_list's applies only if neither moves the atom, nor to
/// the thread-local template (and only in __DATA, see
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
    let segments = |lists: &[SymbolMove]| -> Vec<&'static str> {
        let cut = |seg: &str| seg[..seg.floor_char_boundary(16)].to_string();
        lists.iter().map(|list| &*String::leak(cut(&list.segment))).collect()
    };
    let (rw_segs, ro_segs) = (segments(&args.move_to_rw), segments(&args.move_to_ro));
    let (mut rw_warnings, mut ro_warnings) = (Vec::new(), Vec::new());
    let warning = |obj: usize, id: SymbolId, atom: &Atom, list: &SymbolMove| {
        let place = (atom.place, ctx.symbols[id].value);
        // ld-prime makes the thread-local variables' descriptors and
        // the relative method lists itself.
        let file = match atom.kind {
            "thread-vars" => "tlv-file".to_string(),
            "objc-method-list" => "objc-file".to_string(),
            _ => resolved_file_name(ctx.objs[obj].mf),
        };
        let what = if atom.content == Content::Code { "code" } else { "not code" };
        let msg = format!(
            "cannot move symbol '{}' ({file}) to segment '{}' because symbol is {what} (is {})",
            ctx.symbols[id].name(),
            list.segment,
            atom.kind
        );
        (place, msg)
    };

    for_each_atom_symbol(ctx, |obj, id, atom| {
        // A list entry file:name names the symbol of an object of that
        // leaf name.
        let name = ctx.symbols[id].name().as_bytes();
        let leaf = ctx.objs[obj].mf.name.file_name().map_or(&[][..], |f| f.as_bytes());
        let qualified = [leaf, b":", name].concat();
        let find = |lists: &[SymbolMove]| {
            lists.iter().enumerate().find_map(|(i, list)| {
                let found = match list.symbols.find(name) {
                    -1 => list.symbols.find(&qualified),
                    found => found,
                };
                (found >= 0).then_some((i, found == 1))
            })
        };

        let mut chosen = None;
        if let Some((i, named)) = find(&args.move_to_rw)
            && rw_segs[i] != atom.segment
        {
            if atom.content != Content::Code {
                chosen = Some(Move { option: MoveOption::Rw, segment: rw_segs[i] });
            } else if named {
                rw_warnings.push(warning(obj, id, &atom, &args.move_to_rw[i]));
            }
        }
        if let Some((i, named)) = find(&args.move_to_ro)
            && ro_segs[i] != atom.segment
        {
            if atom.content != Content::Data {
                chosen = chosen.or(Some(Move { option: MoveOption::Ro, segment: ro_segs[i] }));
            } else if named {
                ro_warnings.push(warning(obj, id, &atom, &args.move_to_ro[i]));
            }
        }
        if chosen.is_none()
            && find(&args.dirty_data).is_some()
            && !matches!(atom.kind, "thread-data" | "thread-bss")
        {
            chosen = Some(Move { option: MoveOption::Dirty, segment: "__DATA_DIRTY" });
        }
        if let (Some(m), Some(isec)) = (chosen, atom.isec) {
            moves.entry(isec).or_insert(m);
        }
    });

    ro_warnings.sort_by_key(|&(place, _)| place);
    rw_warnings.sort_by_key(|&(place, _)| place);
    for (_, msg) in ro_warnings.iter().chain(&rw_warnings) {
        crate::warn!("{msg}");
    }
    moves
}

/// Calls `f` with each symbol that names a live atom to ld-prime, with
/// its object (see atom_named).
fn for_each_atom_symbol<'a, E: Target>(
    ctx: &'a Context<E>,
    mut f: impl FnMut(usize, SymbolId, Atom<'a>),
) {
    let commons = common_owners(ctx);
    let rewritten = rewritten_records(ctx);
    for (i, obj) in ctx.objs.iter().enumerate() {
        if !obj.is_alive || ctx.is_internal(i) {
            continue;
        }
        for (nlist, &id) in obj.nlists.iter().zip(&obj.symbols) {
            if let Some(atom) = atom_named(ctx, &commons, &rewritten, i, nlist, id) {
                f(i, id, atom);
            }
        }
    }
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

/// The atom ld-prime names by the symbol `id` of object `obj`, whose
/// entry is `nlist`: a definition the link kept, external or local but
/// no assembler label (see symtab::keep_local_symbol_in), in a section
/// or absolute; or a common symbol, if the object's tentative definition
/// is the one its subsection stands for (see common_owners). A method
/// list ld-prime rewrote in the relative form (see rewritten_records)
/// is code to it, which its own objc-file holds.
fn atom_named<'a, E: Target>(
    ctx: &'a Context<E>,
    commons: &hashbrown::HashMap<u32, u32>,
    rewritten: &hashbrown::HashMap<u32, bool>,
    obj: usize,
    nlist: &NList,
    id: SymbolId,
) -> Option<Atom<'a>> {
    let sym = &ctx.symbols[id];
    if nlist.is_common() {
        let isec = sym.input_section().filter(|isec| commons.get(isec) == Some(&(obj as u32)))?;
        let (content, kind) = (Content::Data, "common");
        let place = (obj as u32, isec as u64);
        return Some(Atom { isec: Some(isec), place, segment: "__DATA", content, kind });
    }
    if nlist.is_stab()
        || !matches!(nlist.n_type(), N_SECT | N_ABS)
        || sym.file() != Some(FileId::Obj(obj as u32))
    {
        return None;
    }
    let demoted = nlist.n_type & N_PEXT != 0;
    let isec = sym.input_section();
    if !nlist.is_extern() && !keep_local_symbol_in(ctx, sym.name(), isec, demoted, false) {
        return None;
    }
    let Some(isec) = isec else {
        let (content, kind) = (Content::Data, "data");
        let place = (obj as u32, u64::MAX);
        return Some(Atom { isec: None, place, segment: "", content, kind });
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
    Some(Atom {
        isec: movable.then_some(kept as u32),
        place: (file, isec as u64),
        segment: hdr.segname(),
        content,
        kind,
    })
}
