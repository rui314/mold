//! The output sections: which output section each input section goes
//! to, under what name and with what flags (ld-prime's rules for both),
//! the sections the linker synthesizes, and the order of the sections
//! and segments in the file.

use std::os::unix::ffi::OsStrExt;
use std::sync::Mutex;

use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::sectcreate::{InputPlace, SectCreateInput, SectCreateSection};
use crate::chunks::{
    self, ChunkHeader, ChunkId, OutputSection, OutputSectionId, OutputSegment, Tail,
};
use crate::context::Context;
use crate::error;
use crate::error::RawPath;
use crate::error::raw;
use crate::fatal;
use crate::input_files::{FileId, ObjcImageInfo};
use crate::input_files::{is_class_or_protocol_ref_name, standard_section_flags};
use crate::input_sections::{InputSection, InputSectionId};
use crate::macho::*;
use crate::objc::DataBlob;
use crate::symbol_moves::{Move, MoveOption};
use crate::util::align_to;
use crate::util::worker_local::WorkerLocal;

/// The segment for read-only-after-fixup data: __DATA_CONST unless
/// -no_data_const.
pub(crate) fn data_seg<E: Target>(ctx: &Context<E>) -> &'static [u8] {
    if ctx.args.data_const { b"__DATA_CONST" } else { b"__DATA" }
}

/// Sections a final link places in __DATA_CONST: data that needs no
/// writes after dyld's fixups. ld-prime's list - signed pointers
/// (__auth_ptr), CF and ObjC constant objects, the ObjC lists and the
/// initializer lists - but for the ones only a condition moves (see
/// SectionMap::const_name). A section not on it, such as
/// __objc_boolobj, stays in __DATA.
const DATA_CONST_SECTIONS: &[&[u8]] = &[
    b"__auth_ptr",
    b"__cfstring",
    b"__const",
    b"__const_cfobj2",
    b"__got",
    b"__mod_init_func",
    b"__mod_term_func",
    b"__objc_arraydata",
    b"__objc_arrayobj",
    b"__objc_dateobj",
    b"__objc_dictobj",
    b"__objc_doubleobj",
    b"__objc_floatobj",
    b"__objc_intobj",
    b"__objc_catlist",
    b"__objc_catlist2",
    b"__objc_classlist",
    b"__objc_imageinfo",
    b"__objc_nlcatlist",
    b"__objc_nlclslist",
    b"__objc_protolist",
];

/// Class, protocol and superclass references are written by the
/// Objective-C runtime on older systems, so they stay in __DATA unless
/// the deployment target is macOS 14.4, iOS 17.4, visionOS 1.1 or
/// later, where ld-prime moves them to __DATA_CONST (dyld fixes them up
/// there; from the next releases on most class references fold into
/// __got, see objc::fold_objc_classrefs).
fn objc_refs_are_const<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.targets(&crate::macho::VERSION_2024_SPRING)
}

/// dyld reads an image's interposing tuples (__DATA,__interpose) but
/// never writes them, so from macOS 15, iOS 18 and visionOS 2 on
/// ld-prime makes them read-only after fixups in any image dyld loads:
/// they go to __DATA_CONST, even with -no_data_const. (A -r output, no
/// image, keeps a -sectcreate __DATA,__interpose in __DATA.)
fn interpose_is_const<E: Target>(ctx: &Context<E>) -> bool {
    !ctx.args.relocatable
        && ctx.args.targets(&crate::macho::VERSION_2024_FALL)
        && !ctx.args.without_dyld()
}

/// An output section's name: (segment, section).
type SectionName = (&'static [u8], &'static [u8]);

/// The output section an input section with `flags` lands in, and the
/// name its flags follow; None for one the link consumes or drops.
/// `args` gives -rename_section and -rename_segment.
///
/// The __LLVM segment (bitcode, __swift_modhash, __cmdline, __asm) is
/// copied into no output, and __objc_clsrolist, a compiler-to-linker
/// list of the class_ro_t records of generic Swift classes (nothing
/// references it), into no image. ld-prime names the rest in three
/// steps. Its own moves come first (see SectionMap::builtin_name), so
/// -rename_section matches __DATA_CONST,__const, not __DATA,__const;
/// then -rename_section and -rename_segment rename that name (see
/// SectionMap::renamed for __interpose's move, which comes last); and a
/// section the renames leave in place then merges as in ld64 -
/// __StaticInit into __text, the fixed-size literal pools
/// (__literal4/8/16, already merged per element) into __const - under
/// that section's renamed name: a section renamed __literal8 is not one
/// of the pools.
/// The flags follow the name before the renames: a renamed
/// __objc_classlist is still a list the runtime scans, a renamed
/// __literal8 still a literal pool. A -r output keeps every section
/// as it came, but for the renames.
fn output_section_for(
    args: &crate::cmdline::Args,
    map: SectionMap,
    segname: &[u8],
    sectname: &[u8],
    flags: u32,
) -> Option<(SectionName, SectionName)> {
    if segname == b"__LLVM" {
        return None;
    }
    let name = map.zero_fill_name((static_name(segname), static_name(sectname)), flags);
    if map.relocatable {
        return Some((renamed(args, name), name));
    }
    if name == (b"__DATA", b"__objc_clsrolist") {
        return None;
    }
    let name = map.builtin_name(name, flags);
    let out = map.renamed(args, name);
    Some(match merged_name(name) {
        Some(merged) if out == name => (renamed(args, merged), merged),
        _ => (out, name),
    })
}

/// The section a final link merges a __TEXT section into, like ld64:
/// __StaticInit joins __text, and the literal pools join __const.
fn merged_name(name: (&[u8], &[u8])) -> Option<SectionName> {
    match name {
        (b"__TEXT", b"__StaticInit") => Some((b"__TEXT", b"__text")),
        (b"__TEXT", b"__literal4" | b"__literal8" | b"__literal16") => {
            Some((b"__TEXT", b"__const"))
        }
        _ => None,
    }
}

/// Applies -rename_section and then -rename_segment to a section's
/// name, as ld-prime does: the first -rename_section naming the
/// section renames it, and the first -rename_segment naming the
/// resulting segment then moves it - after a -rename_section too, so
/// a section renamed into a renamed segment moves on. Neither applies
/// twice: -rename_section chains A to B and B to C take A to B.
pub(crate) fn renamed(args: &crate::cmdline::Args, name: SectionName) -> SectionName {
    let (seg, sect) = section_renamed(args, name);
    (renamed_segment(args, seg), sect)
}

/// The name -rename_section gives a section (see renamed).
fn section_renamed(args: &crate::cmdline::Args, name: SectionName) -> SectionName {
    let (seg, sect) = name;
    match args.rename_sections.iter().find(|(s, t, _, _)| s == seg && t == sect) {
        Some((_, _, s, t)) => (static_name(s), static_name(t)),
        None => name,
    }
}

/// The segment -rename_segment moves a segment's sections to.
fn renamed_segment(args: &crate::cmdline::Args, seg: &'static [u8]) -> &'static [u8] {
    match args.rename_segments.iter().find(|(old, _)| old == seg) {
        Some((_, new)) => static_name(new),
        None => seg,
    }
}

/// A section or segment name that lives as long as the output's
/// headers: the usual segment names are literals, and the rest are
/// leaked (the callers name each distinct section once).
fn static_name(name: &[u8]) -> &'static [u8] {
    match name {
        b"__TEXT" => b"__TEXT",
        b"__DATA_CONST" => b"__DATA_CONST",
        b"__DATA" => b"__DATA",
        _ => crate::util::leak_bytes(name.to_vec()),
    }
}

/// What decides where output_section_for puts an input section.
#[derive(Clone, Copy)]
struct SectionMap {
    relocatable: bool,
    data_const: bool,
    objc_const_refs: bool,
    const_interpose: bool,
    const_selrefs: bool,
    shared_region: bool,
    relative_methods: bool,
    text_exec: bool,
    merge_zero_fill: bool,
}

impl SectionMap {
    /// The name ld-prime gives an input section of a final image, with
    /// the section's `flags`, before -rename_section and
    /// -rename_segment: with -text_exec (an arm64 kext) every section
    /// of code - pure instructions, in any segment - moves into
    /// __TEXT_EXEC,__text, and data that needs no writes after fixups
    /// to __DATA_CONST - as do the non-lazy symbol pointers and the
    /// initializer and terminator lists of a __DATA section of any
    /// name, which ld-prime knows by their types (see
    /// output_section_flags).
    fn builtin_name(self, name: SectionName, flags: u32) -> SectionName {
        if self.text_exec && flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
            return (b"__TEXT_EXEC", b"__text");
        }
        if !is_standard_section(name.0, name.1, flags) {
            let is_const = matches!(
                flags & SECTION_TYPE,
                S_NON_LAZY_SYMBOL_POINTERS | S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS
            );
            if name.0 == b"__DATA" && is_const && self.data_const {
                return (b"__DATA_CONST", name.1);
            }
            return name;
        }
        self.const_name(name)
    }

    /// A standard __DATA section's name in a final image when it needs
    /// no writes after dyld's fixups: the same section in __DATA_CONST,
    /// unless -no_data_const - in the shared region, where dyld fixes
    /// them up for good, the Objective-C runtime's class data too; and
    /// the selector references as Args::const_selrefs says. ld-prime
    /// treats this move as a renaming, which boundary symbols follow as
    /// well (unlike -text_exec's: section$start$__TEXT$__text stays in
    /// __TEXT).
    fn const_name(self, name: SectionName) -> SectionName {
        let (seg, sect) = name;
        let is_const = match sect {
            b"__objc_classrefs" | b"__objc_protorefs" | b"__objc_superrefs" => self.objc_const_refs,
            b"__objc_selrefs" => self.const_selrefs,
            // Unless it holds absolute method lists, which the runtime
            // sorts in place.
            b"__objc_const" => self.shared_region && self.relative_methods,
            _ => DATA_CONST_SECTIONS.contains(&sect),
        };
        if seg == b"__DATA" && self.data_const && is_const { (b"__DATA_CONST", sect) } else { name }
    }

    /// The name a symbol move (see symbol_moves) gives a subsection of
    /// the input section `seg`,`sect` with `flags`, before
    /// -rename_section and -rename_segment rename it, and the name its
    /// flags follow: -move_to_rw_segment and -move_to_ro_segment move
    /// it, before ld-prime's own moves (which then don't apply), to the
    /// section of its name in their segment; -dirty_data_list after
    /// those, out of __DATA alone (None for a section elsewhere), to
    /// __DATA_DIRTY.
    fn moved_name(
        self,
        m: Move,
        seg: &[u8],
        sect: &[u8],
        flags: u32,
    ) -> Option<(SectionName, SectionName)> {
        let name = self.zero_fill_name((static_name(seg), static_name(sect)), flags);
        let from = match m.option {
            MoveOption::Rw | MoveOption::Ro => name,
            MoveOption::Dirty => self.builtin_name(name, flags),
        };
        if m.option == MoveOption::Dirty && from.0 != b"__DATA" {
            return None;
        }
        Some(((m.segment, from.1), from))
    }

    /// The name of a section with the type in `flags` under
    /// -merge_zero_fill_sections, which merges every zero-fill section
    /// of a segment into its __zerofill, in a final image and a -r
    /// output alike, before any rename. (ld-prime merges the
    /// thread-local ones too, and then crashes laying out the
    /// thread-local template.)
    fn zero_fill_name(self, name: SectionName, flags: u32) -> SectionName {
        if self.merge_zero_fill && matches!(flags & SECTION_TYPE, S_ZEROFILL | S_GB_ZEROFILL) {
            (name.0, b"__zerofill")
        } else {
            name
        }
    }

    /// The section a section$start$ or section$end$ symbol names: the
    /// one an input section of that name lands in - or, for a pointer
    /// section only the linker makes, where ld-prime puts it: its GOTs
    /// in __DATA_CONST and, in the shared region, its lazy pointers
    /// too. (An input section of one of those names is data like any
    /// other to ld-prime, which rejects one typed as pointers.)
    fn boundary_name(self, name: SectionName) -> SectionName {
        let is_const = match name {
            (b"__DATA", b"__auth_got" | b"__weak_got" | b"__weak_auth_got") => true,
            (b"__DATA", b"__la_symbol_ptr" | b"__lazy_load_got") => self.shared_region,
            _ => false,
        };
        if self.data_const && is_const { (b"__DATA_CONST", name.1) } else { self.const_name(name) }
    }

    /// A final image's section name after -rename_section and
    /// -rename_segment (see renamed). ld-prime moves the interposing
    /// tuples to __DATA_CONST (see interpose_is_const) in place of a
    /// -rename_section: one naming __DATA,__interpose keeps the section
    /// out of __DATA_CONST, one naming __DATA_CONST,__interpose never
    /// applies, and -rename_segment moves the section on from there.
    /// The move takes any section of that name, -sectcreate's too, but
    /// not one a -rename_section gives the name.
    fn renamed(self, args: &crate::cmdline::Args, name: SectionName) -> SectionName {
        let (seg, sect) = self.renamed_section(args, name);
        (renamed_segment(args, seg), sect)
    }

    /// renamed but for -rename_segment.
    fn renamed_section(self, args: &crate::cmdline::Args, name: SectionName) -> SectionName {
        let is_renamed = args.rename_sections.iter().any(|(s, t, _, _)| s == name.0 && t == name.1);
        if name == (b"__DATA", b"__interpose") && self.const_interpose && !is_renamed {
            return (b"__DATA_CONST", name.1);
        }
        section_renamed(args, name)
    }

    fn new<E: Target>(ctx: &Context<E>) -> Self {
        Self {
            relocatable: ctx.args.relocatable,
            data_const: ctx.args.data_const,
            objc_const_refs: objc_refs_are_const(ctx),
            const_interpose: interpose_is_const(ctx),
            const_selrefs: ctx.args.const_selrefs,
            shared_region: ctx.args.shared_region,
            relative_methods: ctx.args.objc_relative_method_lists,
            text_exec: ctx.args.text_exec,
            merge_zero_fill: ctx.args.merge_zero_fill_sections,
        }
    }

    /// A final link's mapping, for the records the linker synthesizes.
    fn final_link<E: Target>(ctx: &Context<E>) -> Self {
        Self { relocatable: false, ..Self::new(ctx) }
    }
}

/// The flags an output section carries, from the flags ld-prime reads
/// its first member as having (see input_section_flags). A final image
/// keeps only the section types its readers act on - zero fill
/// (S_GB_ZEROFILL is plain zero fill there), strings and literals,
/// initializer and terminator lists, the thread-local kinds, DOF, if
/// bare, and non-lazy symbol pointers, a GOT of the input's whose slots
/// the indirect symbol table names (see indirect_symtab) - and makes
/// the rest regular: coalesced data, and the lazy pointers, stubs,
/// interposing tuples and init offsets only the linker makes in an
/// image. Of the attributes it keeps only that code (a regular or
/// coalesced section of pure instructions) is instructions, which
/// debuggers and disassemblers read: no_dead_strip, live_support,
/// strip_static_syms and no_toc direct the linker, and
/// some_instructions alone is but the assembler's note that it
/// emitted an instruction into the section. A -r output is input to
/// another link, so ld-prime copies the type and attributes verbatim,
/// but for superclass and protocol references of the literal-pointer
/// type, which take the flags of the standard section of their name -
/// and mold makes __DATA,__got regular data there, its relocations
/// kept: a __got of non-lazy pointers needs the indirect symbol table
/// to name its slots, and an object that has one is refused as input
/// (ld-prime writes one, dropping the relocations), while a regular
/// __got is GOT slots to either linker all the same.
fn output_section_flags(segname: &[u8], sectname: &[u8], input: u32, relocatable: bool) -> u32 {
    if relocatable {
        if (segname, sectname) == (b"__DATA", b"__got") {
            return input & !SECTION_TYPE;
        }
        return match standard_section_flags(segname, sectname) {
            Some(table)
                if input & SECTION_TYPE == S_LITERAL_POINTERS
                    && is_class_or_protocol_ref_name(sectname) =>
            {
                table
            }
            _ => input,
        };
    }
    let ty = match input & SECTION_TYPE {
        S_GB_ZEROFILL => S_ZEROFILL,
        ty @ (S_ZEROFILL
        | S_CSTRING_LITERALS
        | S_4BYTE_LITERALS
        | S_8BYTE_LITERALS
        | S_16BYTE_LITERALS
        | S_MOD_INIT_FUNC_POINTERS
        | S_MOD_TERM_FUNC_POINTERS
        | S_NON_LAZY_SYMBOL_POINTERS
        | S_THREAD_LOCAL_REGULAR
        | S_THREAD_LOCAL_ZEROFILL
        | S_THREAD_LOCAL_VARIABLES) => ty,
        S_DTRACE_DOF if input == S_DTRACE_DOF => S_DTRACE_DOF,
        _ => S_REGULAR,
    };
    let is_code = input & S_ATTR_PURE_INSTRUCTIONS != 0
        && matches!(input & SECTION_TYPE, S_REGULAR | S_COALESCED);
    if is_code { ty | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS } else { ty }
}

/// Whether ld-prime places an input section of a standard name (see
/// standard_section_flags) as that standard section: one of the
/// table's type, or of any type for the Objective-C runtime's sections
/// and __got, which it knows by name. Only such a section moves to
/// __DATA_CONST in a final image; any other, such as a __mod_init_func
/// assembled without its type, stays where data of its name goes.
fn is_standard_section(segname: &[u8], sectname: &[u8], flags: u32) -> bool {
    let Some(table) = standard_section_flags(segname, sectname) else {
        return false;
    };
    table & SECTION_TYPE == flags & SECTION_TYPE
        || sectname.starts_with(b"__objc_")
        || (segname, sectname) == (b"__DATA", b"__got")
}

/// The flags ld-prime reads an input section as having, from its
/// canonical ones (see canonical_section_flags): those its table holds
/// for the section's name (see standard_section_flags) if the section
/// has the table's type - a __TEXT,__const or __DATA,__data an
/// assembler nop landed in is plain data again, a regular __text
/// code - and its own otherwise (a regular __cstring holds no literals
/// to merge).
fn input_section_flags(segname: &[u8], sectname: &[u8], flags: u32) -> u32 {
    match standard_section_flags(segname, sectname) {
        Some(table) if table & SECTION_TYPE == flags & SECTION_TYPE => table,
        _ => flags,
    }
}

/// The output section named `name` - created with `flags` if no input
/// made one - with a synthesized `tail` of `tail_size` bytes appended
/// after its input subsections, which are placed already.
fn tail_section<E: Target>(
    ctx: &mut Context<E>,
    name: SectionName,
    flags: u32,
    p2align: u32,
    tail: Tail,
    tail_size: u64,
) -> OutputSectionId {
    let id = find_output_section(ctx, name)
        .unwrap_or_else(|| add_output_section(ctx, name.0, name.1, flags));
    append_tail(ctx.output_section_mut(id), p2align, tail, tail_size);
    id
}

/// Appends a synthesized `tail` of `tail_size` bytes aligned to
/// 2^`p2align` to an output section, after its input subsections.
fn append_tail(osec: &mut OutputSection, p2align: u32, tail: Tail, tail_size: u64) {
    osec.hdr.p2align = osec.hdr.p2align.max(p2align);
    osec.tail = tail;
    osec.tail_off = align_to(osec.hdr.size, 1 << p2align);
    osec.hdr.size = osec.tail_off + tail_size;
}

/// Where an input section goes: the output section's name, the name its
/// flags follow (see output_section_for), and the symbol move that
/// took it there, if one did.
#[derive(Clone, Copy)]
struct Destination {
    name: SectionName,
    flags_name: SectionName,
    moved: Option<MoveOption>,
}

/// Where an input section with header `hdr` goes: to the section the
/// symbol move `m` of its subsection takes it to (see
/// SectionMap::moved_name), if one does, and to its output section
/// (see output_section_for) otherwise; None for one the link consumes.
fn destination(
    args: &crate::cmdline::Args,
    map: SectionMap,
    hdr: &MachSection,
    m: Option<Move>,
) -> Option<Destination> {
    let (seg, sect) = (hdr.segname(), hdr.sectname());
    if let Some(m) = m
        && let Some((moved, flags_name)) = map.moved_name(m, seg, sect, hdr.flags)
    {
        return Some(Destination { name: renamed(args, moved), flags_name, moved: Some(m.option) });
    }
    let (name, flags_name) = output_section_for(args, map, seg, sect, hdr.flags)?;
    Some(Destination { name, flags_name, moved: None })
}

/// Adds the output section `dest` names, for its first member, an
/// input section with header `hdr` (see first_member_flags).
fn add_output_section_for<E: Target>(
    ctx: &mut Context<E>,
    hdr: &MachSection,
    text: SectionName,
    dest: Destination,
) -> OutputSectionId {
    let flags = first_member_flags(ctx, hdr, text, dest.name, dest.flags_name);
    let id = add_output_section(ctx, dest.name.0, dest.name.1, flags);
    ctx.output_section_mut(id).moved = dest.moved;
    id
}

/// The output section of a record the linker rewrote in place of a
/// subsection of the input section `hdr`, as the input's would go (see
/// destination), under the symbol move `m`. The section is made here
/// if no input subsection went there, or every one was replaced.
fn record_section<E: Target>(
    ctx: &mut Context<E>,
    hdr: &MachSection,
    text: SectionName,
    m: Option<Move>,
) -> Option<OutputSectionId> {
    let dest = destination(&ctx.args, SectionMap::final_link(ctx), hdr, m)?;
    let id = find_output_section(ctx, dest.name);
    Some(id.unwrap_or_else(|| add_output_section_for(ctx, hdr, text, dest)))
}

/// A record category merging rewrites in place of an input subsection,
/// such as a class's ro data, takes that subsection's position among
/// its output section's members, as ld-prime keeps
/// __OBJC_CLASS_RO_$_Foo where the input had it - in the section a
/// symbol move takes it to, if one does (see symbol_moves). Runs while
/// the members are still in input order; the other synthesized records
/// go in the section's tail.
fn place_replacing_blobs<E: Target>(
    ctx: &mut Context<E>,
    text: SectionName,
    moves: &hashbrown::HashMap<u32, Move>,
) {
    let blobs: hashbrown::HashSet<u32> = ctx.data_blobs.iter().map(|b| b.isec).collect();
    let mut anchors: Vec<(u32, u32)> = (0..ctx.isecs.len())
        .filter(|&i| blobs.contains(&ctx.isecs[i].replacement))
        .map(|i| (i as u32, ctx.isecs[i].replacement))
        .collect();
    let mut seen = hashbrown::HashSet::new();
    anchors.retain(|&(_, blob)| seen.insert(blob));
    // Last first: a blob inserted (with its high index) then only ever
    // sits after the members the next, lower anchor is searched among.
    for (replaced, blob) in anchors.into_iter().rev() {
        let hdr = *ctx.hdr_of(&ctx.isecs[replaced as usize]);
        let Some(id) = record_section(ctx, &hdr, text, moves.get(&blob).copied()) else {
            continue;
        };
        let p2align = ctx.isecs[blob as usize].p2align as u32;
        let osec = ctx.output_section_mut(id);
        let at = osec.members.partition_point(|&m| m < replaced);
        osec.members.insert(at, blob);
        osec.has_blobs = true;
        osec.hdr.p2align = osec.hdr.p2align.max(p2align);
        ctx.isecs[blob as usize].set_output_section(ChunkId::Output(id));
    }
}

/// The synthesized Objective-C records not placed among the inputs go
/// in the tail of the section they name.
fn place_tail_blobs<E: Target>(ctx: &mut Context<E>) {
    let unplaced =
        |ctx: &Context<E>, b: &DataBlob| ctx.isecs[b.isec as usize].output_section().is_none();
    let mut sects: Vec<&'static [u8]> =
        ctx.data_blobs.iter().filter(|b| unplaced(ctx, b)).map(|b| b.sect).collect();
    sects.sort();
    sects.dedup();
    for sect in sects {
        let map = SectionMap::final_link(ctx);
        let ((seg, out), _) = output_section_for(&ctx.args, map, b"__DATA", sect, 0).unwrap();
        // Each record at its own alignment (a pointer's, but for the
        // lazy-load flag words), the tail at the first one's; laid out
        // from where the tail will start, so that the offsets within
        // the section are aligned.
        let blobs: Vec<(u32, u64, u32)> = (ctx.data_blobs.iter())
            .filter(|b| b.sect == sect && unplaced(ctx, b))
            .map(|b| (b.isec, b.size(), ctx.isecs[b.isec as usize].p2align as u32))
            .collect();
        let first = blobs[0].2;
        let start = find_output_section(ctx, (seg, out))
            .map_or(0, |id| align_to(ctx.output_section(id).hdr.size, 1 << first));
        let mut end = start;
        let mut offs = Vec::new();
        for &(isec, size, p2align) in &blobs {
            end = align_to(end, 1 << p2align);
            offs.push((isec, end));
            end += size;
        }
        let size = end - start;
        let id = tail_section(ctx, (seg, out), S_REGULAR, first, Tail::DataBlobs, size);
        let osec = ctx.output_section_mut(id);
        osec.hdr.p2align = blobs.iter().map(|b| b.2).fold(osec.hdr.p2align, u32::max);
        for (isec, off) in offs {
            ctx.isecs[isec as usize].set_output_section(ChunkId::Output(id));
            ctx.isecs[isec as usize].offset = off as u32;
        }
    }
}

/// Creates the output sections: assigns each input section to its
/// output section, adds the sections the linker synthesizes, sorts them
/// all into file order and groups them into segments.
pub fn create_output_sections<E: Target>(ctx: &mut Context<E>) {
    ctx.chunks.push(ChunkId::MachHeader);
    let text = text_section_name(ctx);
    let moves = crate::symbol_moves::find_moves(ctx);
    assign_input_sections(ctx, text, &moves);
    place_replacing_blobs(ctx, text, &moves);
    place_sectcreate_inputs(ctx);

    set_section_alignments(ctx);
    sort_section_members(ctx);
    compute_section_sizes(ctx);

    // The sections the linker synthesizes.
    add_stub_and_got_chunks(ctx);
    if !ctx.init_offsets.init_funcs.is_empty() {
        chunks::init_offsets::update_shdr(ctx);
        ctx.chunks.push(ChunkId::InitOffsets);
    }
    add_objc_stubs(ctx);
    place_tail_blobs(ctx);
    lay_out_objc_method_lists(ctx, text, &moves);
    add_sectcreate_sections(ctx);
    merge_objc_image_info(ctx);
    if ctx.args.fixup_chains_section {
        ctx.chain_starts.hdr.reserved1 = ctx.args.chain_starts_kind;
        ctx.chunks.push(ChunkId::ChainStarts);
    }
    if ctx.args.unwind_info() && chunks::unwind_info::is_needed(ctx) {
        ctx.chunks.push(ChunkId::UnwindInfo);
    }
    lay_out_eh_frame(ctx);
    warn_eh_frame_too_large(ctx);
    add_linkedit_chunks(ctx);
    rename_synthetic_sections(ctx);
    add_boundary_sections(ctx);
    trace_symbol_layout(ctx);

    sort_chunks(ctx);
    create_segments(ctx);
    add_boundary_segments(ctx);
    add_stack_segment(ctx);
    finish_section_alignments(ctx, text);
    crate::chunks::indirect_symtab::assign_indices(ctx);
    check_segment_order(ctx);
    check_section_order(ctx);
    check_interposing(ctx);
    // The mach header's segment must come first after __PAGEZERO. A
    // -static image's header moves with -rename_segment __TEXT, and
    // only -segment_order can then put its segment there.
    if ctx.chunks.first() != Some(&ChunkId::MachHeader) {
        fatal!("Invalid -segment_order, __TEXT must be the first segment after zero page");
    }
    if ctx.args.no_zero_fill_sections && !ctx.args.relocatable {
        fill_zero_fill_sections(ctx);
    }
}

/// Appends each live input section to its output section (see
/// output_section_for) in input order - a subsection a symbol move
/// takes to another segment to the section that names (see
/// SectionMap::moved_name) - creating the output sections in the order
/// their first members come, and drops the sections the link consumes.
/// A final image's sections that renames made of zero-fill and
/// file-backed members alike are then settled.
///
/// The members are found in parallel and gathered by block of the arena
/// (see group_input_sections), and then, as in mold's
/// create_output_sections, each output section's groups are concatenated
/// in input order. mold sorts its sections by name; here they are made
/// in the order of their first members, as ld-prime orders them, which
/// a -r output keeps.
fn assign_input_sections<E: Target>(
    ctx: &mut Context<E>,
    text: SectionName,
    moves: &hashbrown::HashMap<u32, Move>,
) {
    let (block_groups, table) = group_input_sections(ctx, moves);

    // Transpose only the groups that exist, retaining input order.
    let mut grouped: Vec<Vec<&OutputSectionFileMembers>> =
        (0..table.fill_kinds.len()).map(|_| Vec::new()).collect();
    for groups in &block_groups {
        for (section, members) in groups {
            grouped[*section].push(members);
        }
    }

    // Make the output sections in the order their first members come,
    // each with the flags and the move of its first member (see
    // add_output_section_for).
    let mut order: Vec<usize> = (0..grouped.len()).collect();
    order.sort_by_key(|&section| grouped[section][0].members[0]);
    let mut ids = vec![OutputSectionId::new(0); grouped.len()];
    for section in order {
        let first = grouped[section][0];
        let hdr = *ctx.hdr_of(&ctx.isecs[first.members[0]]);
        ids[section] = add_output_section_for(ctx, &hdr, text, first.dest);
        ctx.output_section_mut(ids[section]).has_tlv_data = table.has_tlv_data[section];
    }

    // Copy large member vectors in parallel, as well as flattening
    // different output sections in parallel.
    let flattened: Vec<(OutputSectionId, Vec<InputSectionId>, u8)> = grouped
        .into_par_iter()
        .enumerate()
        .map(|(section, parts)| {
            let n = parts.iter().map(|g| g.members.len()).sum();
            let mut members = vec![0; n];
            let mut rest = members.as_mut_slice();
            let mut slices = Vec::with_capacity(parts.len());
            for g in &parts {
                slices.push(rest.split_off_mut(..g.members.len()).unwrap());
            }
            parts.par_iter().zip(slices).for_each(|(g, slice)| {
                slice.copy_from_slice(&g.members);
            });
            let p2align = parts.iter().map(|g| g.p2align).max().unwrap_or(0);
            (ids[section], members, p2align)
        })
        .collect();
    for (id, members, p2align) in flattened {
        let osec = ctx.output_section_mut(id);
        osec.members = members;
        osec.hdr.p2align = osec.hdr.p2align.max(p2align as u32);
    }

    // Point the members at their output sections, a block at a time.
    ctx.isecs.par_chunks_mut(BLOCK).zip(&block_groups).for_each(|(isecs, groups)| {
        for (section, group) in groups {
            for &i in &group.members {
                isecs[i as usize % BLOCK].set_output_section(ChunkId::Output(ids[*section]));
            }
        }
    });

    if !ctx.args.relocatable {
        let mut fill_kinds = vec![0; ctx.output_sections.len()];
        for (section, &kinds) in table.fill_kinds.iter().enumerate() {
            fill_kinds[ids[section].index()] = kinds;
        }
        resolve_zerofill_conflicts(ctx, &fill_kinds);
    }
}

/// The input sections group_input_sections hands each job: blocks of
/// the arena take the place of mold's files, as the subsections of all
/// the files are in one arena.
const BLOCK: usize = 4096;

/// Finds the output section of each live input section in parallel, as
/// mold's create_output_sections does, through a cache per worker and a
/// table shared by all (see OutputSectionTable), where the output
/// sections are numbered as the workers come to them. Returns the
/// table, and each block's members by output section, in input order;
/// drops the input sections the link consumes.
fn group_input_sections<E: Target>(
    ctx: &mut Context<E>,
    moves: &hashbrown::HashMap<u32, Move>,
) -> (Vec<Vec<(usize, OutputSectionFileMembers)>>, OutputSectionTable) {
    let map = SectionMap::new(ctx);

    // Keep a cache per worker so it is reused across Rayon jobs. Each
    // mutex is locked once per block, without contention between workers.
    let shared = Mutex::new(OutputSectionTable::default());
    let caches = WorkerLocal::new(WorkerCache::default);

    let Context { isecs, objs, args, .. } = ctx;
    let block_groups = isecs
        .par_chunks_mut(BLOCK)
        .enumerate()
        .map(|(block, isecs)| {
            let mut groups: Vec<(usize, OutputSectionFileMembers)> = Vec::new();
            let mut cache = caches.get();
            // The subsections of an input section are contiguous in the
            // arena and go to the same output section, but for those a
            // symbol move takes elsewhere: the last input section's
            // (file, shndx) and group spare all but its first subsection
            // the key's hash - on a debug link, millions of lookups.
            let mut last: Option<((u32, u32), Option<usize>)> = None;
            for (i, isec) in (block * BLOCK..).zip(isecs) {
                if !isec.is_emitted() || isec.is_placed() {
                    continue;
                }
                let sec = (isec.file, isec.shndx);
                let mv = moves.get(&(i as u32)).copied();
                let group = match last {
                    Some((last_sec, group)) if last_sec == sec && mv.is_none() => group,
                    _ => {
                        let hdr = &objs[isec.file as usize].sect_hdrs[isec.shndx as usize];
                        let key = output_section_key(hdr, mv);
                        let section = *cache
                            .sections
                            .entry(key)
                            .or_insert_with(|| shared.lock().unwrap().get(args, map, hdr, mv, key));
                        let group = section
                            .map(|(section, dest)| cache.group(&mut groups, block, section, dest));
                        if mv.is_none() {
                            last = Some((sec, group));
                        }
                        group
                    }
                };
                let Some(group) = group else {
                    // Consumed by the link: no output section.
                    isec.set_alive(false);
                    continue;
                };
                let group = &mut groups[group].1;
                group.members.push(i as u32);
                group.p2align = group.p2align.max(isec.p2align);
            }
            groups
        })
        .collect();
    drop(caches);
    (block_groups, shared.into_inner().unwrap())
}

/// What decides an input section's output section: the raw 16-byte
/// names, so that the hot loop does no allocation, the flags, which say
/// whether -text_exec moves it and whether it is the standard section of
/// its name (see is_standard_section), and the move of a moved
/// subsection.
type OutputSectionKey = ([u8; 16], [u8; 16], u32, Option<(MoveOption, &'static [u8])>);

/// The key of the input sections with header `hdr` whose subsections
/// the symbol move `mv` takes, if one does (mold's output_section_key).
fn output_section_key(hdr: &MachSection, mv: Option<Move>) -> OutputSectionKey {
    (hdr.segname, hdr.sectname, hdr.flags, mv.map(|m| (m.option, m.segment)))
}

/// A worker's cache (mold's CachedOutputSection, in two parts, as
/// several keys can lead to one output section): the output section
/// each key leads to, as numbered in OutputSectionTable, and where its
/// input sections go (None for those the link consumes); and for each
/// output section, the block whose members the worker last gathered and
/// its group there.
#[derive(Default)]
struct WorkerCache {
    sections: hashbrown::HashMap<OutputSectionKey, Option<(usize, Destination)>>,
    groups: Vec<Option<(usize, usize)>>,
}

impl WorkerCache {
    /// The group among `groups` of block `block`'s members of output
    /// section `section`, made for the first of them, which goes to
    /// `dest`.
    fn group(
        &mut self,
        groups: &mut Vec<(usize, OutputSectionFileMembers)>,
        block: usize,
        section: usize,
        dest: Destination,
    ) -> usize {
        if self.groups.len() <= section {
            self.groups.resize(section + 1, None);
        }
        if let Some((b, group)) = self.groups[section]
            && b == block
        {
            return group;
        }
        groups.push((section, OutputSectionFileMembers::new(dest)));
        self.groups[section] = Some((block, groups.len() - 1));
        groups.len() - 1
    }
}

/// A block's members of an output section, in input order, with the
/// largest alignment among them and where the first of them goes (mold's
/// OutputSectionFileMembers).
struct OutputSectionFileMembers {
    members: Vec<InputSectionId>,
    p2align: u8,
    dest: Destination,
}

impl OutputSectionFileMembers {
    fn new(dest: Destination) -> Self {
        Self { members: Vec::new(), p2align: 0, dest }
    }
}

/// The output sections the input sections go to, as the workers of
/// group_input_sections find them (mold's OutputSectionShared): by key,
/// then by their (possibly renamed) names, as several keys can land in
/// one output section. They are numbered as found; assign_input_sections
/// makes them in the order of their first members.
#[derive(Default)]
struct OutputSectionTable {
    by_key: hashbrown::HashMap<OutputSectionKey, Option<(usize, Destination)>>,
    by_name: hashbrown::HashMap<SectionName, usize>,
    /// Whether each output section has zero-fill (bit 0) and
    /// file-backed (bit 1) input sections; renames can mix them.
    fill_kinds: Vec<u8>,
    /// Whether each output section has thread-local input sections.
    has_tlv_data: Vec<bool>,
}

impl OutputSectionTable {
    /// The output section, and where the input sections go (see
    /// destination), of the input sections with header `hdr` whose
    /// subsections the symbol move `mv` takes, if one does; None for
    /// those the link consumes.
    fn get(
        &mut self,
        args: &crate::cmdline::Args,
        map: SectionMap,
        hdr: &MachSection,
        mv: Option<Move>,
        key: OutputSectionKey,
    ) -> Option<(usize, Destination)> {
        if let Some(&section) = self.by_key.get(&key) {
            return section;
        }
        let section = destination(args, map, hdr, mv).map(|dest| {
            let n = self.by_name.len();
            let section = *self.by_name.entry(dest.name).or_insert(n);
            if section == n {
                self.fill_kinds.push(0);
                self.has_tlv_data.push(false);
            }
            // The key holds the section type: note what it brings to
            // the output section once.
            self.fill_kinds[section] |= if hdr.is_zerofill() { 1 } else { 2 };
            if matches!(hdr.section_type(), S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL) {
                self.has_tlv_data[section] = true;
            }
            (section, dest)
        });
        self.by_key.insert(key, section);
        section
    }
}

/// -trace_symbol_layout prints, or -trace_symbol_layout_file writes,
/// the output section each symbol the map lists (see
/// mapfile::is_map_symbol) went to, in symbol order: "symbol '_x',
/// mapped to __TEXT/__text". A file that can't be
/// written is a warning, ending with a blank line as ld-prime's does,
/// and the trace goes nowhere then. A -r link reports nothing.
fn trace_symbol_layout<E: Target>(ctx: &Context<E>) {
    let args = &ctx.args;
    if args.relocatable {
        return;
    }
    let mut out: Box<dyn std::io::Write> = match &args.trace_symbol_layout_file {
        Some(path) => match std::fs::File::create(path) {
            Ok(file) => Box::new(std::io::BufWriter::new(file)),
            Err(e) => {
                crate::warn!(
                    "could not open -trace_symbol_layout_file {} for writing ({})\n",
                    path.raw(),
                    e.raw_os_error().unwrap_or(0)
                );
                return;
            }
        },
        None if args.trace_symbol_layout => Box::new(std::io::stdout().lock()),
        None => return,
    };
    for sym in &ctx.symbols.syms {
        let Some(isec) = sym.input_section() else { continue };
        if !crate::mapfile::is_map_symbol(sym) {
            continue;
        }
        let Some(chunk) = ctx.isecs[ctx.resolve_isec(isec as usize)].output_section() else {
            continue;
        };
        let hdr = ctx.chunk_header(chunk);
        let line = [b"symbol '", sym.name(), b"', mapped to ", hdr.segname, b"/", hdr.sectname];
        let _ = out.write_all(&line.concat());
        let _ = out.write_all(b"\n");
    }
}

/// The flags of a new output section, `out`, from those of its first
/// member, an input section with header `hdr` whose flags follow the
/// name `flags_name` (see output_section_for). The first member
/// decides, as in ld-prime: code after data in a section doesn't make
/// it code. An empty member counts if a symbol names a subsection there
/// (see bare_sections), in -r too.
fn first_member_flags<E: Target>(
    ctx: &Context<E>,
    hdr: &MachSection,
    text: SectionName,
    out: SectionName,
    flags_name: SectionName,
) -> u32 {
    let relocatable = ctx.args.relocatable;
    let (seg, sect) = (hdr.segname(), hdr.sectname());
    if !relocatable && out == text {
        // ld-prime makes a final image's __text itself, as code,
        // whatever its members (and under its -rename_section name).
        return S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
    }
    if merged_name((seg, sect)) == Some(flags_name) {
        // A literal pool folded into __const is constants there,
        // whatever its type.
        return S_REGULAR;
    }
    let mut input = input_section_flags(seg, sect, hdr.flags);
    // A kext's pointers are plain data to ld-prime, its GOT's too.
    if ctx.args.is_kext() && input & SECTION_TYPE == S_NON_LAZY_SYMBOL_POINTERS {
        input &= !SECTION_TYPE;
    }
    output_section_flags(flags_name.0, flags_name.1, input, relocatable)
}

/// Adds an empty output section named `seg`,`sect` with `flags`.
fn add_output_section<E: Target>(
    ctx: &mut Context<E>,
    seg: &'static [u8],
    sect: &'static [u8],
    flags: u32,
) -> OutputSectionId {
    let mut osec = OutputSection::new(seg, sect);
    osec.hdr.flags = flags;
    let id = OutputSectionId::new(ctx.output_sections.len() as u32);
    ctx.output_sections.push(osec);
    ctx.chunks.push(ChunkId::Output(id));
    id
}

/// The output section named `name`, if there is one.
fn find_output_section<E: Target>(ctx: &Context<E>, name: SectionName) -> Option<OutputSectionId> {
    let (seg, sect) = name;
    let i = ctx.output_sections.iter().position(|o| o.hdr.segname == seg && o.hdr.sectname == sect);
    i.map(|i| OutputSectionId::new(i as u32))
}

/// Settles the output sections' alignments, which their members raised
/// to the largest of theirs: the thread-local template's sections share
/// the strictest one (see also finish_section_alignments).
fn set_section_alignments<E: Target>(ctx: &mut Context<E>) {
    // The thread-local template (the initial values, __thread_data,
    // followed by the zero fill, __thread_bss) is one image dyld copies
    // per thread, so ld-prime gives all its sections, by type, the
    // strictest of their alignments.
    let in_template = |osec: &OutputSection| {
        matches!(osec.hdr.flags & SECTION_TYPE, S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL)
    };
    let template = ctx.output_sections.iter().filter(|o| in_template(o));
    if let Some(p2align) = template.map(|osec| osec.hdr.p2align).max() {
        for osec in ctx.output_sections.iter_mut().filter(|o| in_template(o)) {
            osec.hdr.p2align = p2align;
        }
    }
}

/// Settles each section's alignment in output order, the linker's own
/// (__stubs, __got, __unwind_info...) as well, and warns about a section
/// in that order as ld-prime does. -sectalign sets the alignment, e.g.
/// to page-align a blob that will be mapped or measured separately;
/// ld64 lowers it too, with a warning: its members keep their offsets
/// within the section, so one may end up misaligned (a fixup that can't
/// reach it then fails). Then a section cannot be aligned beyond its
/// segment's (the page, unless -segalign says otherwise): ld64 reduces
/// the alignment with a warning (an x86-64 .align 16 asks for 64KB),
/// which -no_warn_reduced_section_align silences (not -sectalign's) -
/// but not in a -static or -preload image or a kext, which no dyld
/// maps: ld-prime starts the section's segment on the alignment there
/// (see segment_start_align).
fn finish_section_alignments<E: Target>(ctx: &mut Context<E>, text: SectionName) {
    let capped = !ctx.args.relocatable && !ctx.args.static_link && !ctx.args.is_kext();
    let max = ctx.args.segment_align.max(1).trailing_zeros();
    let warn_capped = ctx.args.warn_reduced_section_align;
    for id in ctx.chunks.clone() {
        let hdr = ctx.chunk_header(id);
        if !hdr.is_sect {
            continue;
        }
        let sectalign = ctx
            .args
            .sectalign
            .iter()
            .find(|(seg, sect, _)| hdr.segname == seg && hdr.sectname == *sect);
        let sectalign = sectalign.map(|&(_, _, p2align)| p2align as u32);
        let hdr = ctx.chunk_header_mut(id);
        if let Some(p2align) = sectalign {
            if p2align < hdr.p2align {
                crate::warn!(
                    "-sectalign reduces alignment of {},{} from {} to {}",
                    raw(hdr.segname),
                    raw(hdr.sectname),
                    1u64 << hdr.p2align,
                    1u64 << p2align
                );
            }
            hdr.p2align = p2align;
        }
        if capped && hdr.p2align > max {
            if warn_capped {
                crate::warn!(
                    "reducing alignment of section {},{} from 0x{:x} to 0x{:x} because it exceeds segment maximum alignment",
                    raw(hdr.segname),
                    raw(hdr.sectname),
                    1u64 << hdr.p2align,
                    1u64 << max
                );
            }
            hdr.p2align = max;
        }
    }

    // dyld wants its code at a stable address, whatever its load
    // commands take: ld64 aligns its __text to 4 KiB, on any target and
    // whatever the inputs or -sectalign ask, and leaves no room between
    // the load commands and it (see chunks::header_pad).
    if ctx.args.is_dylinker()
        && let Some(id) = find_output_section(ctx, text)
    {
        ctx.output_sections[id.index()].hdr.p2align = 12;
    }
}

/// Orders each output section's members: the subsections -order_file
/// names first, cold code last, and the rest in input order (see
/// assign_input_sections).
fn sort_section_members<E: Target>(ctx: &mut Context<E>) {
    // -order_file moves the subsections it names to the front of their
    // output sections, in the file's order; everything else keeps its
    // input order behind them. A stable sort by rank does both.
    if let Some(ranks) = order_file_ranks(ctx) {
        for osec in &mut ctx.output_sections {
            osec.members.sort_by_key(|&id| ranks[id as usize]);
        }
    }

    // Cold code last: clang marks the rarely-run part it splits off a
    // function (foo.cold.1, and the function it came from) N_COLD_FUNC,
    // and ld64 lays those subsections out after every other subsection
    // of their section - in final images and -r outputs alike - so hot
    // code stays dense.
    let mut cold = vec![false; ctx.isecs.len()];
    let mut any = false;
    for obj in &ctx.objs {
        if !obj.is_alive {
            continue;
        }
        for (msym, &sym_id) in obj.mach_syms.iter().zip(&obj.symbols) {
            if msym.is_stab() || msym.ty() != N_SECT || msym.desc & N_COLD_FUNC == 0 {
                continue;
            }
            if let Some(isec) = ctx.symbols[sym_id].input_section() {
                cold[isec as usize] = true;
                any = true;
            }
        }
    }
    if any {
        for osec in &mut ctx.output_sections {
            osec.members.sort_by_key(|&id| cold[id as usize]);
        }
    }
}

/// Computes each input section's offset within its output section, and
/// the output sections' sizes. Following mold's design, sections lay
/// out in parallel: each output section's offsets depend only on its
/// own members, so the per-section prefix sums run on all cores and the
/// results are written back serially. Code gets range-extension thunks
/// later, if a branch can be out of reach at all, once the order of the
/// sections is known (see thunks.rs).
fn compute_section_sizes<E: Target>(ctx: &mut Context<E>) {
    let offsets: Vec<(usize, Vec<u64>, u64)> = ctx
        .output_sections
        .par_iter()
        .enumerate()
        .map(|(i, osec)| {
            let mut offs = Vec::with_capacity(osec.members.len());
            let mut off = 0;
            for &id in &osec.members {
                let isec = &ctx.isecs[id];
                off = isec.align_offset(off);
                offs.push(off);
                off += isec.size as u64;
            }
            (i, offs, off)
        })
        .collect();
    for (i, offs, size) in offsets {
        for (&id, off) in ctx.output_sections[i].members.iter().zip(offs) {
            ctx.isecs[id].offset = off as u32;
        }
        ctx.output_sections[i].hdr.size = size;
    }
}

/// Adds the objc_msgSend$ stubs to the output, and appends their
/// selector strings and reference slots to the sections of those names
/// (as their tail): the Objective-C runtime uniques the selectors of
/// one __objc_selrefs section per image, and a second one would leave
/// every compiler-emitted @selector() unregistered. The input
/// subsections are placed already, so the tail's offset and the
/// section's final size are known here.
fn add_objc_stubs<E: Target>(ctx: &mut Context<E>) {
    if !ctx.objc_stubs.symbols.is_empty() {
        chunks::objc_stubs::update_shdr(ctx);
        ctx.chunks.push(ChunkId::ObjcStubs);
    }

    let methname_size = ctx.objc_stubs.methname_data.len() as u64;
    let selrefs_size =
        (ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len()) as u64 * 8;
    let map = SectionMap::final_link(ctx);
    if methname_size > 0 {
        let (name, _) =
            output_section_for(&ctx.args, map, b"__TEXT", b"__objc_methname", S_CSTRING_LITERALS)
                .unwrap();
        let tail = Tail::ObjcMethname;
        let id = tail_section(ctx, name, S_CSTRING_LITERALS, 0, tail, methname_size);
        ctx.objc_stubs.methname = Some(id);
    }
    if selrefs_size > 0 {
        let (name, _) =
            output_section_for(&ctx.args, map, b"__DATA", b"__objc_selrefs", S_LITERAL_POINTERS)
                .unwrap();
        let tail = Tail::ObjcSelrefs;
        let id = tail_section(ctx, name, S_REGULAR, 3, tail, selrefs_size);
        ctx.objc_stubs.selrefs = Some(id);
    }
}

/// Lays out __objc_methlist, the method lists rewritten in the relative
/// form (see convert_objc_method_lists), in the order they were made,
/// each 8-byte aligned (the class records point at them); category
/// merging also retires some after their first placement. The lists
/// -move_to_ro_segment takes to another segment (see symbol_moves) go
/// alike to an __objc_methlist there.
fn lay_out_objc_method_lists<E: Target>(
    ctx: &mut Context<E>,
    text: SectionName,
    moves: &hashbrown::HashMap<u32, Move>,
) {
    if ctx.objc_methlist.lists.is_empty() {
        return;
    }
    let order: Vec<u32> = ctx.objc_methlist.lists.iter().map(|l| l.isec).collect();

    // The lists of each section, by the section: None for
    // __TEXT,__objc_methlist. (Of the symbol moves, only
    // -move_to_ro_segment's takes code.)
    let mut groups: Vec<(Option<OutputSectionId>, Vec<u32>)> = Vec::new();
    for isec in order {
        let hdr = *ctx.hdr_of(&ctx.isecs[isec as usize]);
        let m = moves.get(&isec).filter(|m| m.option == MoveOption::Ro);
        let dest = m.and_then(|&m| record_section(ctx, &hdr, text, Some(m)));
        match groups.iter_mut().find(|(d, _)| *d == dest) {
            Some((_, lists)) => lists.push(isec),
            None => groups.push((dest, vec![isec])),
        }
    }
    for (dest, lists) in groups {
        let mut off = 0u64;
        for &isec in &lists {
            off = align_to(off, 8);
            ctx.isecs[isec as usize].offset = off as u32;
            off += ctx.isecs[isec as usize].size as u64;
        }
        let (chunk, base) = match dest {
            None => {
                ctx.objc_methlist.hdr.size = off;
                ctx.chunks.push(ChunkId::ObjcMethlist);
                (ChunkId::ObjcMethlist, 0)
            }
            Some(id) => {
                let osec = ctx.output_section_mut(id);
                append_tail(osec, 3, Tail::ObjcMethlists, off);
                (ChunkId::Output(id), osec.tail_off)
            }
        };
        for isec in lists {
            let isec = &mut ctx.isecs[isec as usize];
            isec.offset += base as u32;
            isec.set_output_section(chunk);
        }
    }
}

/// Lays out the input sections of -sectcreate, of a file's contents,
/// and of -add_empty_section, empty, which gives tools a named anchor
/// (its section$start/end addresses), in command-line order. One that
/// names an input section's output section - after -rename_section and
/// -rename_segment - joins it after the input sections, byte-aligned,
/// as a subsection of the internal object: programs read the data back
/// with getsectiondata, which finds the first section of a name. The
/// others make sections of their own, one of each name, their contents
/// in command-line order.
fn place_sectcreate_inputs<E: Target>(ctx: &mut Context<E>) {
    debug_assert!(ctx.sectcreate_sections.is_empty());
    let map = SectionMap::final_link(ctx);
    let mut contents: Vec<Vec<u8>> = Vec::new();
    for i in 0..ctx.args.sectcreate.len() {
        let sc = &ctx.args.sectcreate[i];
        let name = map.renamed(&ctx.args, (static_name(&sc.segname), static_name(&sc.sectname)));
        let data: &'static [u8] = match &sc.path {
            Some(path) => Vec::leak(std::fs::read(path).unwrap_or_else(|e| {
                fatal!("cannot open -sectcreate file {}: {}", path.raw(), error::strerror(&e))
            })),
            None => &[],
        };
        let place = match find_output_section(ctx, name) {
            Some(osec) => InputPlace::Isec(add_sectcreate_isec(ctx, osec, i, data)),
            None => {
                let own = (ctx.sectcreate_sections.iter())
                    .position(|s| s.hdr.segname == name.0 && s.hdr.sectname == name.1);
                let section = own.unwrap_or_else(|| {
                    ctx.sectcreate_sections.push(SectCreateSection::new(name.0, name.1, &[]));
                    contents.push(Vec::new());
                    contents.len() - 1
                });
                let offset = contents[section].len() as u64;
                contents[section].extend_from_slice(data);
                InputPlace::Section { section: section as u32, offset }
            }
        };
        ctx.sectcreate_inputs.push(SectCreateInput { size: data.len() as u64, place });
    }
    for (sec, data) in ctx.sectcreate_sections.iter_mut().zip(contents) {
        sec.hdr.size = data.len() as u64;
        sec.contents = Vec::leak(data);
    }
}

/// Adds -sectcreate option `i`'s input section of `data` to output
/// section `osec`, after its members, as a subsection of the internal
/// object. Returns the subsection.
fn add_sectcreate_isec<E: Target>(
    ctx: &mut Context<E>,
    osec: OutputSectionId,
    i: usize,
    data: &'static [u8],
) -> u32 {
    let sc = &ctx.args.sectcreate[i];
    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: bytes_to_name(&sc.sectname),
        segname: bytes_to_name(&sc.segname),
        size: data.len() as u64,
        ..Default::default()
    });
    let id = ctx.isecs.len() as u32;
    ctx.isecs.push(InputSection {
        output_section: ChunkId::Output(osec).pack(),
        flags: InputSection::flags_placed(),
        ..InputSection::new(file, shndx, 0, data.len() as u32, data)
    });
    ctx.output_section_mut(osec).members.push(id);
    id
}

/// Adds the sections the -sectcreate and -add_empty_section options
/// make (see place_sectcreate_inputs) to the image's chunks.
fn add_sectcreate_sections<E: Target>(ctx: &mut Context<E>) {
    for i in 0..ctx.sectcreate_sections.len() {
        ctx.chunks.push(ChunkId::SectCreate(i as u32));
    }
}

/// Merges the objects' __objc_imageinfo records into the image's, cut
/// to the flags an image keeps, a lone record's too (see
/// passes::objc_image_flags and merge_objc_info), in the order
/// ld-prime checks the objects in (see passes::check_objc_flags, which
/// gave the diagnostics). An image no dyld loads (-static, -preload, a
/// kext), whose Objective-C no runtime sets up, gets none from
/// ld-prime.
fn merge_objc_image_info<E: Target>(ctx: &mut Context<E>) {
    let mut objs: Vec<&crate::input_files::ObjectFile> =
        ctx.objs.iter().filter(|o| o.is_alive && o.objc_image_info.is_some()).collect();
    objs.sort_by_key(|o| o.priority);
    let info = objs
        .iter()
        .filter_map(|o| o.objc_image_info)
        .map(|info| ObjcImageInfo { flags: crate::passes::objc_image_flags(info.flags), ..info })
        .reduce(crate::passes::merge_objc_info);
    let Some(ObjcImageInfo { flags, .. }) = info else { return };
    if ctx.args.without_dyld() {
        return;
    }
    ctx.objc_imageinfo.flags = flags;
    ctx.objc_imageinfo.hdr.segname = data_seg(ctx);
    ctx.objc_imageinfo.hdr.size = 8;
    ctx.chunks.push(ChunkId::ObjcImageInfo);
}

/// Lays out __eh_frame, the surviving DWARF unwind records: the CIEs
/// the kept FDEs use, then the FDEs, as mold's EhFrameSection does (an
/// FDE's CIE pointer is a backward offset). Their offsets are needed
/// before layout, because the __unwind_info encoding embeds each FDE's
/// offset.
fn lay_out_eh_frame<E: Target>(ctx: &mut Context<E>) {
    // FDEs of folded copies duplicate their leader's; drop them, and
    // remap the unwind records' FDE indices around the removals as the
    // dead-strip pass does, so that none points past the shortened
    // table.
    let mut fde_map = vec![usize::MAX; ctx.fdes.len()];
    let mut kept_fdes = Vec::new();
    for (i, fde) in std::mem::take(&mut ctx.fdes).into_iter().enumerate() {
        if ctx.isecs[fde.isec as usize].replacement == crate::input_sections::NO_REPLACEMENT {
            fde_map[i] = kept_fdes.len();
            kept_fdes.push(fde);
        }
    }
    ctx.fdes = kept_fdes;
    let num_records = ctx.unwind_records.len();
    ctx.unwind_records.retain_mut(|rec| {
        if rec.fde_idx == crate::input_files::UNWIND_NONE {
            return true;
        }
        let mapped = fde_map[rec.fde_idx as usize];
        if mapped == usize::MAX {
            // A folded copy's record; its leader has its own.
            return false;
        }
        rec.fde_idx = mapped as u32;
        true
    });
    // The compaction moved the surviving records; refresh the
    // subsections' ranges, which the __unwind_info encoding reads.
    if ctx.unwind_records.len() < num_records {
        crate::input_files::refresh_unwind_ranges(ctx);
    }
    if ctx.fdes.is_empty() {
        return;
    }

    for fde in &ctx.fdes {
        ctx.cies[fde.cie as usize].is_alive = true;
    }
    let mut off = 0;
    for cie in ctx.cies.iter_mut().filter(|cie| cie.is_alive) {
        cie.output_offset = off;
        off += cie.data.len() as u32;
    }
    for fde in &mut ctx.fdes {
        fde.output_offset = off;
        off += fde.data.len() as u32;
    }
    ctx.eh_frame.hdr.size = off as u64;
    ctx.chunks.push(ChunkId::EhFrame);
}

/// Warns, as ld-prime does, if __unwind_info points a function at an
/// FDE beyond the reach of the 24 bits an entry has for its offset
/// (which it leaves 0 then, see encode_unwind_info).
fn warn_eh_frame_too_large<E: Target>(ctx: &Context<E>) {
    use chunks::unwind_info::MAX_FDE_OFFSET;
    if !ctx.args.warn_eh_frame_too_large
        || ctx.eh_frame.hdr.size <= MAX_FDE_OFFSET as u64
        || !ctx.chunks.contains(&ChunkId::UnwindInfo)
    {
        return;
    }
    let out_of_reach = ctx.unwind_records.iter().any(|rec| {
        let isec = &ctx.isecs[rec.isec as usize];
        isec.is_emitted()
            && rec.fde().is_some_and(|fde| ctx.fdes[fde].output_offset > MAX_FDE_OFFSET)
    });
    if out_of_reach {
        crate::warn!(
            "__eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected"
        );
    }
}

/// Sorts the chunks into file order, as mold's sort_output_sections
/// does: by segment, then by the kind of a section within its segment
/// (see section_rank), and in creation order among equals - the output
/// sections of the inputs in the order their first members come, then
/// the ones the linker makes. Segment ranks honor -segment_order, then
/// the standard order; segments stay together, and __LINKEDIT is
/// always last. Zero-fill sections go last in their segment so that
/// they take no file space in the middle of it, and -section_order
/// orders the sections of each kind.
fn sort_chunks<E: Target>(ctx: &mut Context<E>) {
    let mut order = ctx.chunks.clone();
    let mut first_seen: hashbrown::HashMap<&'static [u8], usize> = hashbrown::HashMap::new();
    for &id in &order {
        let n = first_seen.len();
        first_seen.entry(ctx.chunk_header(id).segname).or_insert(n);
    }
    let segment_order = &ctx.args.segment_order;
    let static_link = ctx.args.static_link;
    order.sort_by_key(|&id| {
        let hdr = ctx.chunk_header(id);
        // Code in __TEXT_EXEC follows __TEXT. A kext's __DATA_CONST
        // comes after __DATA, as ld-prime places it. The segments of
        // signed pointers, __AUTH_CONST then __AUTH, go before __DATA,
        // but in an image no loader slides by its fixups (a -static or
        // -preload one); __DATA_DIRTY (see symbol_moves) follows __DATA
        // in an image dyld loads. Other segments follow in the order
        // they first appear - in a -static or -preload image, which
        // knows no __DATA_CONST, that one too (with -data_const or in
        // the shared region).
        let standard = match hdr.segname {
            b"__TEXT" | b"__TEXT_EXEC" => 0,
            b"__DATA_CONST" if !ctx.args.without_dyld() => 1,
            b"__AUTH_CONST" if !static_link => 2,
            b"__AUTH" if !static_link => 3,
            b"__DATA" => 4,
            b"__DATA_CONST" if !static_link => 5,
            b"__DATA_DIRTY" if !ctx.args.without_dyld() => 5,
            _ => 6,
        };
        // -segment_order orders the rest: __TEXT, which holds the
        // mach header, stays first and __LINKEDIT last. A -preload
        // image's header precedes its segments but lies in none, and
        // its __TEXT goes where the list says.
        let seg_rank = match (id, hdr.segname) {
            (ChunkId::MachHeader, _) if ctx.args.preload => 0,
            (_, b"__TEXT") if !ctx.args.preload => 0,
            (_, b"__LINKEDIT") => usize::MAX,
            (_, name) => match segment_order.iter().position(|s| s == name) {
                Some(i) => 1 + i,
                None => 1 + segment_order.len() + standard,
            },
        };
        let seg_rank = (seg_rank, first_seen[hdr.segname]);
        (seg_rank, hdr.is_zerofill(), listed_section_rank(ctx, hdr), section_rank(ctx, id))
    });
    ctx.chunks = order;
}

/// Where a chunk goes among those of its segment: the mach header
/// first, then code, then data. The thread-local template is one block
/// dyld copies for each thread, so its initial values come last among
/// the file-backed sections and its zero fill first among the zero-fill
/// ones (see sort_chunks). An -encryptable image's __oslogstring, which
/// stays unencrypted, follows the rest of __TEXT, the encrypted range
/// (see create_encryption_info_cmd); the code signature ends the file.
fn section_rank<E: Target>(ctx: &Context<E>, id: ChunkId) -> u32 {
    let hdr = ctx.chunk_header(id);
    match id {
        ChunkId::MachHeader => return 0,
        ChunkId::CodeSignature => return u32::MAX,
        _ => {}
    }
    if ctx.args.encryptable && hdr.segname == b"__TEXT" && hdr.sectname == b"__oslogstring" {
        return 5;
    }
    match hdr.flags & SECTION_TYPE {
        S_THREAD_LOCAL_ZEROFILL => 1,
        _ if hdr.flags & S_ATTR_PURE_INSTRUCTIONS != 0 => 2,
        S_THREAD_LOCAL_REGULAR => 4,
        _ => 3,
    }
}

/// Groups the chunks, in file order, into segments, and numbers the
/// sections: a MachSym's sect is the 1-based ordinal of its section in
/// the load commands.
fn create_segments<E: Target>(ctx: &mut Context<E>) {
    let mut segments = Vec::new();
    if ctx.args.pagezero_size > 0 {
        segments.push(OutputSegment::new(b"__PAGEZERO"));
    }
    let mut sect_idx = 1u8;
    for i in 0..ctx.chunks.len() {
        let id = ctx.chunks[i];
        if id == ChunkId::MachHeader && ctx.args.preload {
            continue;
        }
        let segname = ctx.chunk_header(id).segname;
        if segments.last().map(|s: &OutputSegment| s.name) != Some(segname) {
            segments.push(OutputSegment::new(segname));
        }
        segments.last_mut().unwrap().chunks.push(id);
        let hdr = ctx.chunk_header_mut(id);
        if hdr.is_sect {
            hdr.sect_idx = sect_idx;
            sect_idx = sect_idx.wrapping_add(1);
        }
    }
    ctx.segments = segments;
}

/// -no_zero_fill_sections gives every zero-fill section its bytes in
/// the file, for a loader that copies segments from the file without
/// zero-filling them (the x86-64 XNU kernel's booter): ld-prime makes
/// such a section regular, once it has taken its place at the end of
/// its segment. A thread-local one becomes S_THREAD_LOCAL_REGULAR, not
/// S_REGULAR as in ld-prime (which ld64 left thread-local): dyld finds
/// the thread-local template by those two types.
fn fill_zero_fill_sections<E: Target>(ctx: &mut Context<E>) {
    for osec in &mut ctx.output_sections {
        let hdr = &mut osec.hdr;
        let regular = match hdr.flags & SECTION_TYPE {
            S_ZEROFILL => S_REGULAR,
            S_THREAD_LOCAL_ZEROFILL => S_THREAD_LOCAL_REGULAR,
            _ => continue,
        };
        hdr.flags = (hdr.flags & !SECTION_TYPE) | regular;
    }
}

/// Where -section_order puts a section in its segment: the listed
/// sections lead in the list's order, after the mach header and after
/// __text unless the list places it; the rest follow as usual.
fn listed_section_rank<E: Target>(ctx: &Context<E>, hdr: &ChunkHeader) -> usize {
    let Some((_, list)) = ctx.args.section_order.iter().find(|(seg, _)| seg == hdr.segname) else {
        return 0;
    };
    match list.iter().position(|s| *s == hdr.sectname) {
        Some(i) => 1 + i,
        None if !hdr.is_sect || is_text_section(hdr) => 0,
        None => usize::MAX,
    }
}

fn is_text_section(hdr: &ChunkHeader) -> bool {
    hdr.segname == b"__TEXT" && hdr.sectname == b"__text"
}

/// ld-prime refuses a -section_order that puts a zero-fill section, which
/// has no file bytes, ahead of one with contents: the listed sections
/// lead their segment, so a listed zero-fill section must follow every
/// other section with contents, listed or not.
fn check_section_order<E: Target>(ctx: &Context<E>) {
    for (seg, list) in &ctx.args.section_order {
        let sects: Vec<&ChunkHeader> = ctx
            .chunks
            .iter()
            .map(|&id| ctx.chunk_header(id))
            .filter(|hdr| hdr.is_sect && hdr.segname == seg)
            .collect();
        // ld-prime's order: the listed sections, then the others but an
        // unlisted __text, which leads them all.
        let listed = list.iter().filter_map(|name| sects.iter().find(|hdr| hdr.sectname == *name));
        let others = sects
            .iter()
            .filter(|hdr| !list.iter().any(|s| s == hdr.sectname) && !is_text_section(hdr));
        let order: Vec<&&ChunkHeader> = listed.chain(others).collect();
        if let Some(i) = order.iter().position(|hdr| hdr.is_zerofill())
            && order[i..].iter().any(|hdr| !hdr.is_zerofill())
        {
            fatal!(
                "{} is zero-fill, it should be ordered at the end of the segment {}, or alongside other zero-fill sections",
                raw(order[i].sectname),
                raw(seg)
            );
        }
    }
}

/// An image bound for the shared region (see resolve_shared_region)
/// may not carry interposing tuples, which the dyld shared cache
/// builder refuses. ld-prime finds them as dyld does - a section named
/// __interpose in a segment whose name starts with __DATA or __AUTH,
/// by its final name - and rejects even an empty one, naming the last
/// in the image.
fn check_interposing<E: Target>(ctx: &Context<E>) {
    if !ctx.args.shared_region {
        return;
    }
    let is_interpose = |hdr: &&ChunkHeader| {
        hdr.is_sect
            && hdr.sectname == b"__interpose"
            && (hdr.segname.starts_with(b"__DATA") || hdr.segname.starts_with(b"__AUTH"))
    };
    if let Some(hdr) = ctx.chunks.iter().map(|&id| ctx.chunk_header(id)).rfind(is_interpose) {
        error!(
            "Shared cache eligible dylib cannot use interposing tuples (found in '{} {}').  \
             Remove interposing tuples, or opt out of the shared cache using the build setting \
             'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag '-not_for_dyld_shared_cache')",
            raw(hdr.segname),
            raw(hdr.sectname)
        );
    }
}

/// The segment of the mach header: __TEXT, which -rename_segment
/// moves only in a -static image. A dynamic image's header stays in
/// __TEXT, where dyld looks for it.
pub(crate) fn header_segment<E: Target>(ctx: &Context<E>) -> &'static [u8] {
    if ctx.args.static_link { renamed_segment(&ctx.args, b"__TEXT") } else { b"__TEXT" }
}

/// The name of a final image's __text section: it moves
/// with -text_exec like the code, and -rename_section and
/// -rename_segment rename it like any section - but -rename_segment
/// __TEXT leaves it with the mach header.
fn text_section_name<E: Target>(ctx: &Context<E>) -> SectionName {
    let (seg, sect) =
        SectionMap::final_link(ctx).builtin_name((b"__TEXT", b"__text"), S_ATTR_PURE_INSTRUCTIONS);
    let is_renamed = ctx.args.rename_sections.iter().any(|(s, t, _, _)| s == seg && t == sect);
    if seg == b"__TEXT" && !is_renamed {
        (header_segment(ctx), sect)
    } else {
        renamed(&ctx.args, (seg, sect))
    }
}

/// Applies -rename_section and -rename_segment to the sections the
/// linker synthesizes, as ld-prime does to all of them - the stubs and
/// their helper, the GOT and lazy pointers, __init_offsets,
/// __eh_frame, the Objective-C ones and -sectcreate's - but
/// __unwind_info, which stays in __TEXT; and moves the mach header to
/// its segment. (The output sections of input sections, and those of
/// -sectcreate, got their renamed names when created.) A -sectcreate
/// __DATA,__interpose moves to __DATA_CONST like an input section (see
/// SectionMap::renamed).
fn rename_synthetic_sections<E: Target>(ctx: &mut Context<E>) {
    let map = SectionMap::final_link(ctx);
    if ctx.args.rename_sections.is_empty()
        && ctx.args.rename_segments.is_empty()
        && !map.const_interpose
    {
        return;
    }
    ctx.mach_header.hdr.segname = header_segment(ctx);
    for i in 0..ctx.chunks.len() {
        let id = ctx.chunks[i];
        let hdr = ctx.chunk_header(id);
        if !hdr.is_sect
            || matches!(id, ChunkId::Output(_) | ChunkId::UnwindInfo | ChunkId::SectCreate(_))
        {
            continue;
        }
        let (seg, sect) = map.renamed(&ctx.args, (hdr.segname, hdr.sectname));
        let hdr = ctx.chunk_header_mut(id);
        hdr.segname = seg;
        hdr.sectname = sect;
    }
}

/// Resolves each section$start$/section$end$ and segment$start$/
/// segment$end$ symbol to the output section or segment it names, and
/// creates the sections nothing else does. ld-prime renames the name
/// as it does an input section's - __DATA,__const becomes
/// __DATA_CONST,__const, and -rename_section and -rename_segment
/// apply - or as its own section's (see SectionMap::boundary_name),
/// but merges and drops nothing: section$start$__TEXT$__literal8
/// names an empty __literal8 of its own, with the flags of the
/// standard section of its name (see standard_section_flags), if any.
fn add_boundary_sections<E: Target>(ctx: &mut Context<E>) {
    let map = SectionMap::final_link(ctx);
    for i in 0..ctx.boundary_syms.len() {
        let (_, _, seg, sect) = ctx.boundary_syms[i];
        let Some(sect) = sect else {
            ctx.boundary_syms[i].2 = renamed_segment(&ctx.args, seg);
            continue;
        };
        let flags = standard_section_flags(seg, sect).unwrap_or(S_REGULAR);
        let name = map.boundary_name(map.zero_fill_name((seg, sect), flags));
        let (seg, sect) = map.renamed(&ctx.args, name);
        ctx.boundary_syms[i].2 = seg;
        ctx.boundary_syms[i].3 = Some(sect);
        if !ctx.chunks.iter().any(|&id| {
            let hdr = ctx.chunk_header(id);
            hdr.is_sect && hdr.segname == seg && hdr.sectname == sect
        }) {
            let mut sec = SectCreateSection::new(seg, sect, &[]);
            sec.hdr.flags = flags;
            add_sectcreate(ctx, sec);
        }
    }
}

/// Gives a segment$start$ or segment$end$ symbol naming a segment the
/// image lacks - no input has one, or -rename_section emptied it - an
/// empty segment (no sections, vmsize 0) to point at, as ld-prime
/// does: just before __LINKEDIT and at its address, in the order of
/// the symbols' names.
fn add_boundary_segments<E: Target>(ctx: &mut Context<E>) {
    let mut syms: Vec<(&[u8], &'static [u8])> = ctx
        .boundary_syms
        .iter()
        .filter(|(_, _, seg, sect)| sect.is_none() && !ctx.segments.iter().any(|s| s.name == *seg))
        .map(|&(id, _, seg, _)| (ctx.symbols[id].name(), seg))
        .collect();
    syms.sort_unstable();
    let mut missing: Vec<&'static [u8]> = Vec::new();
    for (_, seg) in syms {
        if !missing.contains(&seg) {
            missing.push(seg);
        }
    }
    let linkedit = ctx.segments.len() - 1;
    ctx.segments.splice(linkedit..linkedit, missing.into_iter().map(OutputSegment::new));
}

/// The -stack_size stack of an executable that starts from
/// LC_UNIXTHREAD: a segment of address space alone before __LINKEDIT,
/// pinned where resolve_stack says.
fn add_stack_segment<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.unixthread && ctx.args.stack_size != 0 {
        let linkedit = ctx.segments.len() - 1;
        ctx.segments.insert(linkedit, OutputSegment::new(b"__UNIXSTACK"));
    }
}

/// Adds the stubs, the lazy-binding helper and pointers, the lazy-load
/// helpers and slots, and the GOT to the output, the ones in use, each
/// sized by its update_shdr.
fn add_stub_and_got_chunks<E: Target>(ctx: &mut Context<E>) {
    if !ctx.stubs.symbols.is_empty() {
        chunks::stubs::update_shdr(ctx);
        ctx.chunks.push(ChunkId::Stubs);
    }
    // (A stub bound by weak lookup goes through the GOT; only lazily
    // bound stubs need the helper and lazy pointers.)
    if !ctx.stubs.lazy.is_empty() {
        chunks::stub_helper::update_shdr(ctx);
        ctx.chunks.push(ChunkId::StubHelper);
        chunks::lazy_ptrs::update_shdr(ctx);
        ctx.chunks.push(ChunkId::LazyPtrs);
    }

    // The delay-init stubs and helpers.
    if !ctx.delay_init.stubs.is_empty() {
        chunks::delay_init::update_stubs_shdr(ctx);
        ctx.chunks.push(ChunkId::DelayStubs);
    }
    if !ctx.delay_init.dlopens.is_empty() {
        chunks::delay_init::update_helper_shdr(ctx);
        ctx.chunks.push(ChunkId::DelayHelper);
    }

    // The lazy-load helpers, and their slots.
    if !ctx.lazy_helpers.helpers.is_empty() {
        chunks::lazy_helpers::update_shdr(ctx);
        ctx.chunks.push(ChunkId::LazyHelpers);
    }
    if !ctx.lazy_load_got.slots.is_empty() {
        chunks::lazy_load_got::update_shdr(ctx);
        ctx.chunks.push(ChunkId::LazyLoadGot);
    }

    if !ctx.got.got_syms.is_empty() {
        chunks::got::update_shdr(ctx);
        ctx.chunks.push(ChunkId::Got);
    }
}

/// Adds the __LINKEDIT tables, in ld-prime's order.
fn add_linkedit_chunks<E: Target>(ctx: &mut Context<E>) {
    // What dyld reads. A -static image or a kext has no dyld: a -static
    // one has only the fixups -fixup_chains or -no_fixup_chains asks
    // for (chains, or rebase and weak-bind opcodes, never an export
    // trie), or under -pie local relocations to slide by; a kext has
    // its relocations, by which kmutil links it. Legacy LINKEDIT has
    // dyld slide an image that slides by its local relocations; ld-prime
    // writes an export trie after them, which no load command names.
    if ctx.args.legacy_linkedit {
        if !chunks::rebase_info::is_never_slid(ctx) {
            ctx.chunks.push(ChunkId::LocalRelocs);
        }
        ctx.chunks.push(ChunkId::ExportTrie);
    } else if !ctx.args.without_dyld() {
        ctx.chunks.push(ChunkId::ChainedFixups);
        ctx.chunks.push(ChunkId::RebaseInfo);
        ctx.chunks.push(ChunkId::BindInfo);
        ctx.chunks.push(ChunkId::WeakBindInfo);
        ctx.chunks.push(ChunkId::LazyBindInfo);
        ctx.chunks.push(ChunkId::ExportTrie);
    } else if ctx.use_chained_fixups() {
        if !ctx.args.fixup_chains_section {
            ctx.chunks.push(ChunkId::ChainedFixups);
        }
    } else if ctx.args.no_fixup_chains {
        ctx.chunks.push(ChunkId::RebaseInfo);
        ctx.chunks.push(ChunkId::WeakBindInfo);
    } else if ctx.args.pie || ctx.args.is_kext() {
        ctx.chunks.push(ChunkId::LocalRelocs);
    }
    // An empty one marks an image -no_shared_cache_eligible keeps out
    // of the shared cache.
    if ctx.args.shared_region || (ctx.args.shared_cache_marker && !ctx.args.preload) {
        ctx.chunks.push(ChunkId::SplitInfo);
    }
    if !ctx.lazy_load_info.dylibs.is_empty() {
        ctx.chunks.push(ChunkId::LazyLoadInfo);
    }
    ctx.chunks.push(ChunkId::FunctionStarts);
    if ctx.args.data_in_code_info {
        ctx.chunks.push(ChunkId::DataInCode);
    }
    if ctx.args.make_mergeable {
        ctx.chunks.push(ChunkId::MergeableRecord);
    }
    ctx.chunks.push(ChunkId::Symtab);
    if ctx.args.is_kext() || ctx.args.legacy_linkedit {
        ctx.chunks.push(ChunkId::ExternRelocs);
    }
    // (Sized once the sections are in order, see assign_indices.)
    if chunks::indirect_symtab::sections(ctx).next().is_some() {
        ctx.chunks.push(ChunkId::IndirectSymtab);
    }
    ctx.chunks.push(ChunkId::Strtab);
    if ctx.args.adhoc_codesign {
        ctx.chunks.push(ChunkId::CodeSignature);
    }
}

/// Makes each output section that renames fill with both zero-fill and
/// file-backed input sections - bits 0 and 1 of `fill_kinds`, by output
/// section - file-backed, with a warning: its zero-fill members are
/// zeros in the file, and no member's contents are lost. (ld-prime gives
/// the section the type of its first member in its own order, and a
/// zero-fill one drops the others' contents.)
fn resolve_zerofill_conflicts<E: Target>(ctx: &mut Context<E>, fill_kinds: &[u8]) {
    for (i, _) in fill_kinds.iter().enumerate().filter(|&(_, &kinds)| kinds == 3) {
        let hdr = &mut ctx.output_sections[i].hdr;
        let ty = match hdr.flags & SECTION_TYPE {
            S_ZEROFILL | S_GB_ZEROFILL => S_REGULAR,
            S_THREAD_LOCAL_ZEROFILL => S_THREAD_LOCAL_REGULAR,
            ty => ty,
        };
        hdr.flags = (hdr.flags & !SECTION_TYPE) | ty;
        crate::warn!(
            "section {},{} has both zero-fill and file-backed input sections; it is laid out \
             in the file",
            raw(hdr.segname),
            raw(hdr.sectname)
        );
    }
}

/// The object each common symbol's subsection stands for the tentative
/// definition of, by subsection: the one declaring the largest size,
/// the first of equals, as in ld-prime.
pub(crate) fn common_owners<E: Target>(ctx: &Context<E>) -> hashbrown::HashMap<u32, u32> {
    let mut decls: hashbrown::HashMap<crate::symbol::SymbolId, (u64, u32)> =
        hashbrown::HashMap::new();
    for (i, obj) in ctx.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive) {
        let r = obj.global_range();
        for (msym, &sym) in obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]) {
            if !msym.is_stab() && msym.ty() == N_UNDF && msym.is_common() {
                let decl = decls.entry(sym).or_insert((msym.value, i as u32));
                if msym.value > decl.0 {
                    *decl = (msym.value, i as u32);
                }
            }
        }
    }
    // A symbol a real definition took is no common symbol's.
    decls
        .into_iter()
        .filter_map(|(sym, (_, obj))| Some((ctx.symbols[sym].input_section()?, obj)))
        .filter(|&(isec, _)| ctx.is_internal(ctx.isecs[isec as usize].file as usize))
        .collect()
}

/// ld-prime's warnings for a -segment_order that places __TEXT or
/// __LINKEDIT where they cannot go, or leaves segments out (they follow
/// the listed ones in the usual order). The __TEXT of a -preload image
/// holds no mach header, and is ordered like any other segment.
fn check_segment_order<E: Target>(ctx: &Context<E>) {
    let order = &ctx.args.segment_order;
    if order.is_empty() {
        return;
    }
    let (text_pos, text_place) =
        if ctx.args.pagezero_size > 0 { (1, "second") } else { (0, "first") };
    let has_text = !ctx.args.preload && ctx.segments.iter().any(|s| s.name == b"__TEXT");
    if has_text && order.iter().position(|s| s == b"__TEXT").is_some_and(|i| i != text_pos) {
        crate::warn!(
            "-segment_order of __TEXT is ignored, the segment must be ordered {text_place}"
        );
    }
    if order.iter().position(|s| s == b"__LINKEDIT").is_some_and(|i| i != order.len() - 1) {
        crate::warn!("-segment_order of __LINKEDIT is ignored, the segment must be ordered last");
    }
    for seg in &ctx.segments {
        let fixed = match seg.name {
            b"__PAGEZERO" | b"__LINKEDIT" => true,
            b"__TEXT" => !ctx.args.preload,
            _ => false,
        };
        if !fixed && !order.iter().any(|s| s == seg.name) {
            crate::warn!("-segment_order should list all segments, {} is missing", raw(seg.name));
        }
    }
}

/// Adds a synthesized section with fixed contents to the output.
fn add_sectcreate<E: Target>(ctx: &mut Context<E>, sec: SectCreateSection) {
    let idx = ctx.sectcreate_sections.len() as u32;
    ctx.sectcreate_sections.push(sec);
    ctx.chunks.push(ChunkId::SectCreate(idx));
}

/// A line of the -order_file lists: [arch:][object-file:]symbol. An
/// arch qualifier gates the whole line; an object qualifier narrows the
/// match to symbols from that file, by leaf name alone as ld-prime
/// compares it: m.o, or lib.a(m.o) for an archive member, but no longer
/// path. Both are bytes, as names are.
struct OrderEntry {
    name: Vec<u8>,
    file: Option<Vec<u8>>,
}

/// Reads the -order_file lists, #-comments and the lines for other
/// architectures left out.
fn read_order_files<E: Target>(ctx: &Context<E>) -> Vec<OrderEntry> {
    use crate::util::{split_once, trim_space};
    const ARCHS: [&[u8]; 6] = [b"arm64", b"arm64e", b"x86_64", b"i386", b"armv7", b"ppc"];
    let mut entries = Vec::new();
    for path in &ctx.args.order_files {
        // ld64 links on without the order a missing file would give.
        let text = match std::fs::read(path) {
            Ok(text) => text,
            Err(e) => {
                crate::warn!("cannot open order file {}: {}", path.raw(), error::strerror(&e));
                continue;
            }
        };
        for line in crate::util::lines(&text) {
            let mut line = trim_space(line.split(|&c| c == b'#').next().unwrap_or_default());
            if line.is_empty() {
                continue;
            }
            if let Some((first, rest)) = split_once(line, b':')
                && ARCHS.contains(&trim_space(first))
            {
                if trim_space(first) != E::NAME.as_bytes() {
                    continue;
                }
                line = trim_space(rest);
            }
            let (file, name) = match split_once(line, b':') {
                Some((file, name)) => (Some(trim_space(file).to_vec()), trim_space(name)),
                None => (None, line),
            };
            entries.push(OrderEntry { name: name.to_vec(), file });
        }
    }
    entries
}

/// Ranks every subsection by the -order_file lists: the subsection the
/// first line names gets rank 0 and so on; unlisted subsections rank
/// last. A line names the live subsections of the symbols of its name
/// (of its object, if it names one), and a subsection takes the rank of
/// the first line that names it. A symbol of the object LTO compiled
/// counts as the bitcode file's it came from, if that is known (see
/// lto::origins), unless -no_use_lto_filenames_in_order_file_matching
/// says to take the object's own name, lto.o. -order_file_statistics
/// reports the lines that name nothing.
fn order_file_ranks<E: Target>(ctx: &Context<E>) -> Option<Vec<u64>> {
    if ctx.args.order_files.is_empty() {
        return None;
    }
    let entries = read_order_files(ctx);
    // A name's lines, as (object, rank).
    type Lines<'a> = Vec<(Option<&'a [u8]>, u64)>;
    let mut rank_of: hashbrown::HashMap<&[u8], Lines> = hashbrown::HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        rank_of.entry(&entry.name).or_default().push((entry.file.as_deref(), i as u64));
    }

    let origins = if !ctx.lto_objs.is_empty() && ctx.args.lto_filenames_in_order_file {
        crate::lto::origins(&ctx.lto_inputs)
    } else {
        hashbrown::HashMap::new()
    };
    let mut ranks = vec![u64::MAX; ctx.isecs.len()];
    let mut found = vec![false; entries.len()];
    for sym in &ctx.symbols.syms {
        let (Some(FileId::Obj(obj)), Some(isec)) = (sym.file(), sym.input_section()) else {
            continue;
        };
        let Some(lines) = rank_of.get(sym.name()) else {
            continue;
        };
        let isec = ctx.resolve_isec(isec as usize);
        if !ctx.isecs[isec].is_alive() {
            continue;
        }
        let mut obj = obj as usize;
        if ctx.is_lto_obj(obj)
            && let Some(&Some(origin)) = origins.get(sym.name())
        {
            obj = origin;
        }
        let leaf = ctx.objs[obj].mf.name.file_name().map_or(&[][..], |f| f.as_bytes());
        for &(_, rank) in lines.iter().filter(|(file, _)| file.is_none_or(|f| leaf == f)) {
            ranks[isec] = ranks[isec].min(rank);
            found[rank as usize] = true;
        }
    }
    if ctx.args.order_file_statistics {
        report_order_file_statistics(&entries, &found);
    }
    Some(ranks)
}

/// -order_file_statistics: warns about each line that names no symbol
/// (see order_file_ranks), and tells how many did.
fn report_order_file_statistics(entries: &[OrderEntry], found: &[bool]) {
    let mut missing = 0;
    for (entry, _) in entries.iter().zip(found).filter(|&(_, &found)| !found) {
        crate::warn!("can't find function/data for order_file entry: {}", raw(&entry.name));
        missing += 1;
    }
    if missing > 0 {
        crate::warn!(
            "only {} out of {} order_file symbols were applicable",
            entries.len() - missing,
            entries.len()
        );
    }
}
