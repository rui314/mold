//! The output sections: which output section each input section goes
//! to, under what name and with what flags (ld-prime's rules for both),
//! the sections the linker synthesizes, and the order of the sections
//! and segments in the file.

use std::os::unix::ffi::OsStrExt;

use rayon::prelude::*;

use crate::chunks::sectcreate::{InputPlace, SectCreateInput, SectCreateSection};
use crate::chunks::{
    self, ChunkHeader, ChunkId, OutputSection, OutputSectionId, OutputSegment, Tail,
};
use crate::context::Context;
use crate::error;
use crate::error::RawPath;
use crate::error::raw;
use crate::fatal;
use crate::input_files::FileId;
use crate::input_sections::InputSection;
use crate::macho::*;
use crate::objc::{DataBlob, cstring_of};
use crate::passes::{is_class_or_protocol_ref_name, resolved_file_name};
use crate::symbol_moves::{Move, MoveOption};
use crate::target::Target;
use crate::util::align_to;

/// Where a section with `flags` sits within its segment in a final
/// image, as ld-prime 27037 orders them; sections of one rank keep
/// input order. ld-prime has one order for all segments, which places
/// the read-only data of __DATA_CONST and the writable data of __DATA
/// alike - so that under -no_data_const (or in an x86-64 kext) the
/// sections that would have made __DATA_CONST lead __DATA in their
/// usual order, but for the GOT and __auth_ptr, which close it. Code
/// comes first in any segment, __text ahead; a section of initializer
/// or terminator pointers, or of thread-local data, has the place of
/// its type in any segment and whatever its name. A -static or
/// -preload image, which no dyld loads, has no place of their own for
/// the initializer and terminator lists, __const, __cfstring and
/// __auth_ptr: they keep input order among the unknown sections there.
///
/// The names that have places are those of ld-prime's standard
/// sections (and crt1.o's), which it knows in an input's __TEXT and
/// __DATA alone: `name` is the one a section ranks by (see rank_name),
/// None for a section that ranks by its type alone.
fn output_section_rank(name: Option<(&[u8], &[u8])>, flags: u32, static_link: bool) -> u32 {
    const UNKNOWN: u32 = 32;
    let dyld = !static_link;
    let (segname, sectname) = name.unwrap_or_default();
    if (segname, sectname) == (b"__TEXT", b"__text") {
        return 0;
    }
    if flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
        return 1;
    }
    match flags & SECTION_TYPE {
        S_MOD_INIT_FUNC_POINTERS if dyld => return 12,
        S_MOD_TERM_FUNC_POINTERS if dyld => return 13,
        // The thread-local template must be contiguous: its initial
        // values come last among file-backed sections, after the
        // variables' descriptors and after the GOT, and its zero fill
        // first among the zero-fill ones (zero-fill sections sort after
        // all file-backed ones).
        S_THREAD_LOCAL_VARIABLES => return 33,
        S_THREAD_LOCAL_REGULAR => return 36,
        S_THREAD_LOCAL_ZEROFILL => return 0,
        // A DTrace DOF section comes after all others of __TEXT,
        // -sectcreate's too, but before __unwind_info (see sort_chunks).
        S_DTRACE_DOF => return 97,
        _ => {}
    }
    match segname {
        b"__TEXT" => match sectname {
            b"__stubs" => 3,
            b"__stub_helper" => 4,
            b"__delay_stubs" => 5,
            b"__delay_helper" => 6,
            b"__lazy_helpers" => 7,
            b"__objc_stubs" => 8,
            b"__init_offsets" => 9,
            b"__objc_methlist" => 10,
            _ => UNKNOWN,
        },
        b"__DATA" | b"__DATA_CONST" => match sectname {
            // crt1.o's tables for dyld and the C runtime, in input
            // order, ahead of the lazy pointers.
            b"__dyld" | b"__program_vars" if segname == b"__DATA" => 2,
            b"__la_symbol_ptr" => 11,
            b"__const" if dyld => 14,
            b"__cfstring" if dyld => 15,
            b"__objc_classlist" => 16,
            b"__objc_nlclslist" => 17,
            b"__objc_catlist" => 18,
            b"__objc_catlist2" => 19,
            b"__objc_nlcatlist" => 20,
            b"__objc_protolist" => 21,
            b"__objc_imageinfo" => 22,
            b"__objc_const" => 23,
            b"__weak_got" => 24,
            b"__objc_selrefs" => 25,
            b"__objc_protorefs" => 26,
            b"__objc_classrefs" => 27,
            b"__objc_superrefs" => 28,
            b"__objc_ivar" => 29,
            b"__objc_data" => 30,
            b"__lazy_load_got" => 31,
            b"__got" => 34,
            b"__auth_ptr" if dyld => 35,
            // ld-prime keeps a place for -merge_zero_fill_sections's
            // __zerofill, whether or not given, ahead of the other
            // zero-fill sections.
            b"__zerofill" if flags & SECTION_TYPE == S_ZEROFILL => 1,
            // The rest, __data among them, and the other zero-fill
            // sections, __bss and __common too, go in first-seen
            // order: the synthesized __common counts from the first
            // object with a common symbol.
            _ => UNKNOWN,
        },
        _ => UNKNOWN,
    }
}

/// Where ld-prime moves a __TEXT section of an image bound for the
/// shared region: the stubs and the Objective-C names, which the shared
/// cache builder bypasses and uniques, come after __unwind_info (100),
/// __eh_frame (101) and an encryptable image's __oslogstring (102), the
/// names in a fixed order. (__oslogstring goes there in any image: its
/// page is left unencrypted.)
fn late_text_rank<E: Target>(ctx: &Context<E>, hdr: &crate::chunks::ChunkHeader) -> Option<u32> {
    if hdr.segname != b"__TEXT" {
        return None;
    }
    if ctx.args.encryptable && hdr.sectname == b"__oslogstring" {
        return Some(102);
    }
    if !ctx.args.shared_region {
        return None;
    }
    match hdr.sectname {
        b"__objc_stubs" => Some(103),
        b"__stubs" => Some(104),
        b"__objc_classname" => Some(105),
        b"__objc_methname" => Some(106),
        b"__objc_methtype" => Some(107),
        _ => None,
    }
}

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
/// the deployment target is macOS 14.4 or later, where ld-prime moves
/// them to __DATA_CONST (dyld fixes them up there; from macOS 15 on
/// most class references fold into __got, see fold_objc_classrefs).
pub(crate) fn objc_refs_are_const<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.platform == crate::macho::PLATFORM_MACOS
        && ctx.args.platform_minos >= crate::macho::encode_version(14, 4, 0)
}

/// dyld reads an image's interposing tuples (__DATA,__interpose) but
/// never writes them, so from macOS 15 on ld-prime makes them read-only
/// after fixups in any image dyld loads: they go to __DATA_CONST, even
/// with -no_data_const. (A -r output, no image, keeps a -sectcreate
/// __DATA,__interpose in __DATA.)
fn interpose_is_const<E: Target>(ctx: &Context<E>) -> bool {
    !ctx.args.relocatable
        && ctx.args.platform == crate::macho::PLATFORM_MACOS
        && ctx.args.platform_minos >= crate::macho::encode_version(15, 0, 0)
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
/// that section's renamed name, if it is the standard section of its
/// name (see is_standard_section): a regular __literal8 holds no
/// literals, and a section renamed __literal8 is not one of the pools.
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
        Some(merged) if out == name && is_standard_section(segname, sectname, flags) => {
            (renamed(args, merged), merged)
        }
        _ => (out, name),
    })
}

/// Whether a final image copies an input section into no output
/// section, as the link consumes it: the __LLVM segment's sections and
/// __objc_clsrolist (see output_section_for).
pub(crate) fn is_consumed_in_image(segname: &[u8], sectname: &[u8]) -> bool {
    segname == b"__LLVM" || (segname == b"__DATA" && sectname == b"__objc_clsrolist")
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
/// twice: -rename_section chains A to B and B to C take A to B. A
/// section of a legacy name no -rename_section names takes its modern
/// name in its place (see modern_name).
pub(crate) fn renamed(args: &crate::cmdline::Args, name: SectionName) -> SectionName {
    let (seg, sect) = section_renamed(args, name);
    (renamed_segment(args, seg), sect)
}

/// The name -rename_section gives a section (see renamed).
fn section_renamed(args: &crate::cmdline::Args, name: SectionName) -> SectionName {
    let (seg, sect) = name;
    match args.rename_sections.iter().find(|(s, t, _, _)| s == seg && t == sect) {
        Some((_, _, s, t)) => (static_name(s), static_name(t)),
        None => modern_name(name),
    }
}

/// The name ld-prime gives a section of a name old compilers used for
/// coalesced (weak) code and data, which lives in the usual sections
/// now: __textcoal_nt is __text, __const_coal __const and
/// __datacoal_nt __data, in the segments ld-prime knows the old names
/// in. It renames them in a final image and a -r output alike, and a
/// boundary symbol's section too, but the flags and the __DATA_CONST
/// move follow the old name: a __DATA,__const_coal stays in __DATA.
fn modern_name(name: SectionName) -> SectionName {
    match name {
        (b"__TEXT", b"__textcoal_nt") => (b"__TEXT", b"__text"),
        (b"__TEXT" | b"__DATA" | b"__DATA_CONST", b"__const_coal") => (name.0, b"__const"),
        (b"__DATA" | b"__DATA_DIRTY", b"__datacoal_nt") => (name.0, b"__data"),
        _ => name,
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
pub(crate) fn static_name(name: &[u8]) -> &'static [u8] {
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
/// its first member as having (see input_section_flags). In a final
/// image ld-prime keeps only the section types it lays out as such -
/// zero fill (S_GB_ZEROFILL is plain zero fill there), strings and
/// literals, initializer and terminator lists, the thread-local kinds,
/// DOF, if bare, and non-lazy symbol pointers, a GOT of the input's
/// whose slots it names in the indirect symbol table (see
/// indirect_symtab) - and makes the rest regular: coalesced data, and
/// the lazy pointers, stubs, interposing tuples and init offsets only
/// it makes in an image. It marks code (a regular or coalesced section
/// of pure instructions) as having some instructions, drops every
/// other input attribute - no_dead_strip, live_support,
/// strip_static_syms and no_toc direct the linker, not dyld, and
/// some_instructions alone is but the assembler's note that it
/// emitted an instruction into the section - and marks just the ObjC
/// list sections the runtime scans, and the class references while in
/// __DATA, as no-dead-strip. Its rules for the Objective-C runtime's
/// sections hold for the standard sections of their names (`standard`,
/// see is_standard_section), in __DATA: an input's
/// __DATA_CONST,__objc_classlist or __DATA_CONST,__objc_selrefs is data
/// like any other. A -r output is input to another link, so ld-prime
/// copies the type and attributes verbatim - but mold makes __DATA,__got
/// regular data there, its relocations kept: a __got of non-lazy
/// pointers needs the indirect symbol table to name its slots, and an
/// object that has one is refused as input (ld-prime writes one,
/// dropping the relocations), while a regular __got is GOT slots to
/// either linker all the same. __eh_frame carries the compiler's fixed
/// flags in both.
fn output_section_flags(
    segname: &[u8],
    sectname: &[u8],
    input: u32,
    standard: bool,
    relocatable: bool,
) -> u32 {
    if segname == b"__TEXT" && sectname == b"__eh_frame" {
        return S_COALESCED | S_ATTR_NO_TOC | S_ATTR_STRIP_STATIC_SYMS | S_ATTR_LIVE_SUPPORT;
    }
    // Superclass and protocol references of the literal-pointer type
    // come out with the flags of the standard section of their name.
    let input = match standard_section_flags(segname, sectname) {
        Some(table)
            if input & SECTION_TYPE == S_LITERAL_POINTERS
                && is_class_or_protocol_ref_name(sectname) =>
        {
            table
        }
        _ => input,
    };
    if relocatable {
        if (segname, sectname) == (b"__DATA", b"__got") {
            return input & !SECTION_TYPE;
        }
        return input;
    }
    // The two reference lists the runtime may still write keep the
    // flags they came with (coalesced, no-dead-strip) while in __DATA
    // of a final image, and the protocol list its coalesced type.
    if standard && segname == b"__DATA" {
        match sectname {
            b"__objc_protorefs" | b"__objc_superrefs" => {
                return input & (SECTION_TYPE | S_ATTR_NO_DEAD_STRIP);
            }
            b"__objc_protolist" => return input & SECTION_TYPE,
            _ => {}
        }
    }
    // ld-prime knows __objc_selrefs by name: its selector references
    // stay literal pointers whatever their type - but those typed so,
    // which it makes plain data once constant (in the shared region),
    // as it does the class references.
    if standard && sectname == b"__objc_selrefs" {
        return if segname == b"__DATA_CONST" && input & SECTION_TYPE == S_LITERAL_POINTERS {
            S_REGULAR
        } else {
            S_LITERAL_POINTERS | S_ATTR_NO_DEAD_STRIP
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
    let mut attrs = 0;
    if input & S_ATTR_PURE_INSTRUCTIONS != 0
        && matches!(input & SECTION_TYPE, S_REGULAR | S_COALESCED)
    {
        attrs = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
    }
    if standard
        && (matches!(
            sectname,
            b"__objc_classlist"
                | b"__objc_catlist"
                | b"__objc_catlist2"
                | b"__objc_nlclslist"
                | b"__objc_nlcatlist"
        ) || (segname == b"__DATA" && sectname == b"__objc_classrefs"))
    {
        attrs |= S_ATTR_NO_DEAD_STRIP;
    }
    ty | attrs
}

/// The flags ld-prime reads a section of an input object as having,
/// which decide how the link splits the section into subsections and
/// what it makes of them - mold's canonicalize_type for a section typed
/// by name alone. __TEXT,__constructor, where GCC put the constructors
/// of code built without dyld (-static, -mkernel) with the assembler's
/// .constructor directive, is a list of initializer pointers whatever
/// its type (__TEXT,__destructor stays data). ld-prime knows the
/// Objective-C runtime's sections by name too (see
/// standard_section_flags): one of another type has the table's flags,
/// so a regular __objc_methname is C strings and a list typed as
/// strings or literals is pointers still. __objc_selrefs keeps its own
/// type, which says whether its references merge, and __DATA,__got,
/// GOT slots whatever its type (see fold_input_got), its own, which
/// says whether the object asks for an indirect-symbol GOT (see
/// check_sections); but neither is ever split into strings or
/// literals. Superclass and protocol references keep the
/// literal-pointer type, whose references all merge (see
/// has_unnamed_subsecs), though the output has the table's flags. Its
/// own flags otherwise.
pub(crate) fn canonical_section_flags(segname: &[u8], sectname: &[u8], flags: u32) -> u32 {
    if (segname, sectname) == (b"__TEXT", b"__constructor") {
        return S_MOD_INIT_FUNC_POINTERS;
    }
    let ty = flags & SECTION_TYPE;
    let is_literal =
        matches!(ty, S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS);
    if (segname, sectname) == (b"__DATA", b"__got") {
        return if is_literal { flags & !SECTION_TYPE } else { flags };
    }
    if !sectname.starts_with(b"__objc_") {
        return flags;
    }
    let Some(table) = standard_section_flags(segname, sectname) else {
        return flags;
    };
    if sectname == b"__objc_selrefs" {
        return if is_literal { flags & !SECTION_TYPE } else { flags };
    }
    if ty == table & SECTION_TYPE
        || (ty == S_LITERAL_POINTERS && is_class_or_protocol_ref_name(sectname))
    {
        flags
    } else {
        table
    }
}

/// Whether ld-prime places an input section of a standard name (see
/// standard_section_flags) as that standard section: one of the
/// table's type, or of any type for the Objective-C runtime's sections
/// and __got, which it knows by name. Only such a section moves to
/// __DATA_CONST or merges into another section in a final image; any
/// other, such as a __mod_init_func or __literal8 assembled without
/// its type, stays where data of its name goes.
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
/// to merge). (A final image has no input __got left: its slots are
/// the GOT's, see fold_input_got.)
fn input_section_flags(segname: &[u8], sectname: &[u8], flags: u32) -> u32 {
    match standard_section_flags(segname, sectname) {
        Some(table) if table & SECTION_TYPE == flags & SECTION_TYPE => table,
        _ => flags,
    }
}

/// The flags of a section ld-prime's table of standard sections names:
/// those a compiler marks a section of that name with, or ld-prime its
/// own sections - code (the stubs and helpers too), literals, pointer
/// lists, the thread-local and zero-fill types, no-dead-strip for the
/// lists the Objective-C runtime scans, and none for the rest of the
/// data. None for another name, or in another segment.
fn standard_section_flags(segname: &[u8], sectname: &[u8]) -> Option<u32> {
    let flags = match (segname, sectname) {
        (
            b"__TEXT",
            b"__text" | b"__StaticInit" | b"__stub_helper" | b"__objc_stubs" | b"__objc_clsstubs"
            | b"__delay_stubs" | b"__delay_helper" | b"__lazy_helpers" | b"__resolver_help",
        ) => S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS,
        (
            b"__TEXT",
            b"__cstring" | b"__objc_classname" | b"__objc_methname" | b"__objc_methtype"
            | b"__oslogstring",
        ) => S_CSTRING_LITERALS,
        (b"__TEXT", b"__literal4") => S_4BYTE_LITERALS,
        (b"__TEXT", b"__literal8") => S_8BYTE_LITERALS,
        (b"__TEXT", b"__literal16") => S_16BYTE_LITERALS,
        (b"__TEXT", b"__eh_frame") => output_section_flags(segname, sectname, 0, true, false),
        (b"__TEXT", b"__const" | b"__ustring" | b"__gcc_except_tab" | b"__objc_methlist") => {
            S_REGULAR
        }
        (b"__DATA", b"__got" | b"__auth_got" | b"__weak_got" | b"__weak_auth_got") => {
            S_NON_LAZY_SYMBOL_POINTERS
        }
        (b"__DATA", b"__la_symbol_ptr" | b"__la_resolver") => S_LAZY_SYMBOL_POINTERS,
        (b"__DATA", b"__mod_init_func") => S_MOD_INIT_FUNC_POINTERS,
        (b"__DATA", b"__mod_term_func") => S_MOD_TERM_FUNC_POINTERS,
        (
            b"__DATA",
            b"__objc_classlist" | b"__objc_nlclslist" | b"__objc_catlist" | b"__objc_catlist2"
            | b"__objc_nlcatlist" | b"__objc_classrefs" | b"__objc_superrefs" | b"__objc_clsrolist",
        ) => S_ATTR_NO_DEAD_STRIP,
        (b"__DATA", b"__objc_protolist") => S_COALESCED,
        (b"__DATA", b"__objc_protorefs") => S_COALESCED | S_ATTR_NO_DEAD_STRIP,
        (b"__DATA", b"__objc_selrefs") => S_LITERAL_POINTERS | S_ATTR_NO_DEAD_STRIP,
        (b"__DATA", b"__thread_vars") => S_THREAD_LOCAL_VARIABLES,
        (b"__DATA", b"__thread_ptrs") => S_THREAD_LOCAL_VARIABLE_POINTERS,
        (b"__DATA", b"__thread_data") => S_THREAD_LOCAL_REGULAR,
        (b"__DATA", b"__thread_bss") => S_THREAD_LOCAL_ZEROFILL,
        (b"__DATA", b"__bss" | b"__common") => S_ZEROFILL,
        (
            b"__DATA",
            b"__data" | b"__const" | b"__cfstring" | b"__auth_ptr" | b"__objc_data"
            | b"__objc_const" | b"__objc_ivar" | b"__objc_imageinfo" | b"__objc_intobj"
            | b"__objc_floatobj" | b"__objc_doubleobj" | b"__objc_dateobj" | b"__objc_dictobj"
            | b"__objc_arrayobj" | b"__objc_arraydata" | b"__const_cfobj2",
        ) => S_REGULAR,
        // The compiler's records for the linker to encode into
        // __unwind_info, which no output carries (but a boundary
        // symbol's empty section).
        (b"__LD", b"__compact_unwind") => S_ATTR_DEBUG,
        _ => return None,
    };
    Some(flags)
}

/// Whether an input subsection is an __objc_methname string that the
/// selector name synthesized for an objc_msgSend$ stub absorbs. ld-prime
/// keeps the synthesized string of the two, so such an input string
/// does not place its output section: when every input string is one,
/// __objc_methname follows every input-derived __TEXT section.
fn is_stub_selector_name<E: Target>(
    ctx: &Context<E>,
    isec: &InputSection,
    stub_sels: &hashbrown::HashSet<&[u8]>,
) -> bool {
    !stub_sels.is_empty()
        && ctx.hdr_of(isec).sectname() == b"__objc_methname"
        && stub_sels.contains(cstring_of(isec.data()))
}

/// The output section named `name` - created if no input made one,
/// with `flags`, ranking by the standard name `rank_name` (see
/// output_section_rank) - with a synthesized `tail` of `tail_size`
/// bytes appended after its input subsections, which are placed
/// already.
fn tail_section<E: Target>(
    ctx: &mut Context<E>,
    name: SectionName,
    rank_name: SectionName,
    flags: u32,
    p2align: u32,
    tail: Tail,
    tail_size: u64,
) -> OutputSectionId {
    let id = match find_output_section(ctx, name) {
        Some(id) => id,
        None => {
            let id = add_output_section(ctx, name.0, name.1, flags);
            ctx.output_section_mut(id).rank_name = Some(rank_name);
            id
        }
    };
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

/// The output section of a record the linker rewrote in place of a
/// subsection of the input section `hdr`, as the input's would go: to
/// the section the symbol move `m` takes it to (see
/// SectionMap::moved_name), if one does. The section is made here if
/// no input subsection went there, or every one was replaced.
fn record_section<E: Target>(
    ctx: &mut Context<E>,
    hdr: &MachSection,
    text: SectionName,
    m: Option<Move>,
) -> Option<OutputSectionId> {
    let map = SectionMap::final_link(ctx);
    let (seg, sect) = (hdr.segname(), hdr.sectname());
    let moved = m.and_then(|m| {
        let (moved, from) = map.moved_name(m, seg, sect, hdr.flags)?;
        Some((m.option, (renamed(&ctx.args, moved), from)))
    });
    let (out, flags_name) = match moved {
        Some((_, names)) => names,
        None => output_section_for(&ctx.args, map, seg, sect, hdr.flags)?,
    };
    if let Some(id) = find_output_section(ctx, out) {
        return Some(id);
    }
    let flags = first_member_flags(ctx, hdr, text, out, flags_name);
    let id = add_output_section(ctx, out.0, out.1, flags);
    let osec = ctx.output_section_mut(id);
    osec.moved = moved.map(|(option, _)| option);
    osec.rank_name = member_rank_name(hdr, flags_name);
    Some(id)
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
        let ((seg, out), flags_name) =
            output_section_for(&ctx.args, map, b"__DATA", sect, 0).unwrap();
        let flags = output_section_flags(flags_name.0, flags_name.1, 0, true, false);
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
        let id = tail_section(ctx, (seg, out), flags_name, flags, first, Tail::DataBlobs, size);
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
    let lto_ranks = lto_layout_ranks(ctx);
    assign_input_sections(ctx, text, &moves, lto_ranks.as_deref());
    place_replacing_blobs(ctx, text, &moves);
    place_sectcreate_inputs(ctx);

    set_section_alignments(ctx);
    sort_section_members(ctx);
    compute_section_sizes(ctx);

    // The sections the linker synthesizes.
    crate::branch_shims::add_far_ref_slots(ctx);
    add_stub_and_got_chunks(ctx);
    if !ctx.init_offsets.init_funcs.is_empty() {
        ctx.init_offsets.hdr.size = ctx.init_offsets.init_funcs.len() as u64 * 4;
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
    merge_same_name_sections(ctx);
    add_boundary_sections(ctx);
    trace_symbol_layout(ctx);

    sort_chunks(ctx, lto_ranks.as_deref());
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
fn assign_input_sections<E: Target>(
    ctx: &mut Context<E>,
    text: SectionName,
    moves: &hashbrown::HashMap<u32, Move>,
    lto_ranks: Option<&[u32]>,
) {
    let map = SectionMap::new(ctx);
    // Each input section name's output section, keyed by the raw
    // 16-byte names, so that the hot loop does no allocation and no
    // linear scans - and by its flags too, which say whether -text_exec
    // moves it and whether it is the standard section of its name (see
    // is_standard_section), and by the move of a moved subsection.
    type Key = ([u8; 16], [u8; 16], u32, Option<(MoveOption, &'static [u8])>);
    let mut by_name: hashbrown::HashMap<Key, Option<OutputSectionId>> = hashbrown::HashMap::new();
    // Output sections by their (possibly renamed) names: several input
    // section names can land in one output section.
    let mut by_out: hashbrown::HashMap<SectionName, OutputSectionId> = hashbrown::HashMap::new();
    // All subsections of one input section share the exact same leaked
    // header pointer and are contiguous in the arena, and a header
    // uniquely names one (object, section) - so a section's whole run
    // of subsections maps to the same output chunk. Cache the last
    // header pointer to skip the 32-byte name hash for all but the
    // first subsection of each section; on a debug link this turns
    // millions of hash lookups into a handful of thousands.
    let mut last_hdr: *const crate::macho::MachSection = std::ptr::null();
    let mut last_osec: Option<OutputSectionId> = None;
    // Whether each output section has zero-fill (bit 0) and
    // file-backed (bit 1) input sections; renames can mix them.
    let mut fill_kinds: Vec<u8> = Vec::new();
    // Input order puts what LTO compiled where its bitcode files were
    // (see lto_layout_ranks), the order the sections appear in too.
    let lto_order = lto_ranks.map(|ranks| {
        let mut ids: Vec<usize> = (0..ctx.isecs.len()).collect();
        ids.sort_by_key(|&i| ranks[i]);
        ids
    });
    for k in 0..ctx.isecs.len() {
        let i = lto_order.as_ref().map_or(k, |ids| ids[k]);
        if !ctx.isecs[i].is_alive()
            || ctx.isecs[i].replacement != crate::input_sections::NO_REPLACEMENT
            || ctx.isecs[i].is_placed()
        {
            continue;
        }
        let hdr_ref = ctx.hdr_of(&ctx.isecs[i]);
        let hdr_ptr = std::ptr::from_ref::<crate::macho::MachSection>(hdr_ref);
        // A copy: the header lives in its object, which stays borrowed
        // while the section is placed below otherwise.
        let hdr = *hdr_ref;
        let mv = moves.get(&(i as u32)).copied();
        let osec_id = if hdr_ptr == last_hdr && mv.is_none() {
            last_osec
        } else {
            let key = (hdr.segname, hdr.sectname, hdr.flags, mv.map(|m| (m.option, m.segment)));
            let id = match by_name.get(&key) {
                Some(&id) => id,
                None => {
                    let (seg, sect) = (hdr.segname(), hdr.sectname());
                    let moved = mv.and_then(|m| {
                        let (moved, from) = map.moved_name(m, seg, sect, hdr.flags)?;
                        Some((m.option, (renamed(&ctx.args, moved), from)))
                    });
                    let out = match moved {
                        Some((_, names)) => Some(names),
                        None => output_section_for(&ctx.args, map, seg, sect, hdr.flags),
                    };
                    let id = out.map(|(out, flags_name)| match by_out.get(&out) {
                        Some(&id) => id,
                        None => {
                            let flags = first_member_flags(ctx, &hdr, text, out, flags_name);
                            let id = add_output_section(ctx, out.0, out.1, flags);
                            let osec = ctx.output_section_mut(id);
                            osec.moved = moved.map(|(option, _)| option);
                            osec.rank_name = member_rank_name(&hdr, flags_name);
                            by_out.insert(out, id);
                            id
                        }
                    });
                    by_name.insert(key, id);
                    id
                }
            };
            if let Some(id) = id {
                if fill_kinds.len() <= id.index() {
                    fill_kinds.resize(id.index() + 1, 0);
                }
                fill_kinds[id.index()] |= if hdr.is_zerofill() { 1 } else { 2 };
                if matches!(hdr.section_type(), S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL) {
                    ctx.output_section_mut(id).has_tlv_data = true;
                }
            }
            if mv.is_none() {
                last_hdr = hdr_ptr;
                last_osec = id;
            }
            id
        };
        let Some(osec_id) = osec_id else {
            // Consumed by the link: no output section.
            ctx.isecs[i].set_alive(false);
            continue;
        };

        let osec = &mut ctx.output_sections[osec_id.index()];
        osec.hdr.p2align = osec.hdr.p2align.max(ctx.isecs[i].p2align as u32);
        osec.members.push(i as u32);
        ctx.isecs[i].set_output_section(ChunkId::Output(osec_id));
    }
    if !ctx.args.relocatable {
        resolve_zerofill_conflicts(ctx, &fill_kinds);
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
    let standard = is_standard_section(seg, sect, hdr.flags);
    let (seg, sect) = flags_name;
    output_section_flags(seg, sect, input, standard, relocatable)
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
/// assign_input_sections). Thread-local zero fill goes by size instead.
fn sort_section_members<E: Target>(ctx: &mut Context<E>) {
    // ld-prime lays out a final image's __thread_bss by subsection size,
    // smallest first and in input order among equals, whatever
    // -order_file says. A subsection's size runs to the next one in its
    // object, padding included.
    let is_tbss = |osec: &OutputSection| osec.hdr.flags & SECTION_TYPE == S_THREAD_LOCAL_ZEROFILL;
    if !ctx.args.relocatable {
        for osec in ctx.output_sections.iter_mut().filter(|osec| is_tbss(osec)) {
            osec.members.sort_by_key(|&id| ctx.isecs[id as usize].size);
        }
    }

    // -order_file moves the subsections it names to the front of their
    // output sections, in the file's order; everything else keeps its
    // input order behind them. A stable sort by rank does both.
    if let Some(ranks) = order_file_ranks(ctx) {
        for osec in ctx.output_sections.iter_mut().filter(|osec| !is_tbss(osec)) {
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
        for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
            if nlist.is_stab() || nlist.n_type() != N_SECT || nlist.n_desc & N_COLD_FUNC == 0 {
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

/// Where a final image lays out what LTO compiled, as ld-prime does:
/// each subsection where the bitcode file it is credited to (see
/// lto::origins) was named, between the inputs before and after that
/// file, and the rest - which stay the compiled objects' - after every
/// input, as the map numbers them. Every other section keeps its place,
/// ranked by the latest input up to it. (-r lays them out after the
/// inputs, as they come.)
pub(crate) fn lto_layout_ranks<E: Target>(ctx: &Context<E>) -> Option<Vec<u32>> {
    if ctx.lto_objs.is_empty() || ctx.args.relocatable {
        return None;
    }
    let mut latest = 0;
    let mut ranks: Vec<u32> = ctx
        .isecs
        .iter()
        .map(|isec| {
            latest = latest.max(ctx.objs[isec.file as usize].priority);
            latest
        })
        .collect();
    let origins = crate::lto::origins(&ctx.lto_inputs);
    for sym in &ctx.symbols.syms {
        if let (Some(FileId::Obj(obj)), Some(isec)) = (sym.file(), sym.input_section())
            && ctx.is_lto_obj(obj as usize)
            && let Some(&Some(origin)) = origins.get(sym.name())
        {
            ranks[isec as usize] = ctx.objs[origin].priority;
        }
    }
    Some(ranks)
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

/// Sizes the objc_msgSend$ stubs, and appends their selector strings and
/// reference slots to the sections of those names (as their tail): the
/// Objective-C runtime uniques the selectors of one __objc_selrefs
/// section per image, and a second one would leave every compiler-
/// emitted @selector() unregistered. The input subsections are placed
/// already, so the tail's offset and the section's final size are
/// known here.
fn add_objc_stubs<E: Target>(ctx: &mut Context<E>) {
    if !ctx.objc_stubs.symbols.is_empty() {
        ctx.objc_stubs.hdr.size = ctx.objc_stubs.symbols.len() as u64 * ctx.objc_stub_size();
        // Code, which -text_exec moves as it does __stubs.
        if ctx.args.text_exec {
            ctx.objc_stubs.hdr.segname = b"__TEXT_EXEC";
        }
        // 32-byte stubs on arm64, small ones word-aligned; ld-prime
        // leaves x86-64's byte-aligned.
        if E::CPUTYPE == crate::macho::CPU_TYPE_X86_64 {
            ctx.objc_stubs.hdr.p2align = 0;
        } else if ctx.args.objc_stubs_small {
            ctx.objc_stubs.hdr.p2align = 2;
        }
        ctx.chunks.push(ChunkId::ObjcStubs);
    }

    let methname_size = ctx.objc_stubs.methname_data.len() as u64;
    let selrefs_size =
        (ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len()) as u64 * 8;
    let map = SectionMap::final_link(ctx);
    if methname_size > 0 {
        let (name, flags_name) =
            output_section_for(&ctx.args, map, b"__TEXT", b"__objc_methname", S_CSTRING_LITERALS)
                .unwrap();
        let flags = S_CSTRING_LITERALS;
        let tail = Tail::ObjcMethname;
        let id = tail_section(ctx, name, flags_name, flags, 0, tail, methname_size);
        ctx.objc_stubs.methname = Some(id);
    }
    if selrefs_size > 0 {
        let (name, flags_name) =
            output_section_for(&ctx.args, map, b"__DATA", b"__objc_selrefs", S_LITERAL_POINTERS)
                .unwrap();
        let flags =
            output_section_flags(flags_name.0, flags_name.1, S_LITERAL_POINTERS, true, false);
        // A slot keeps the alignment of the inputs it took over.
        let p2align = (ctx.objc_stubs.absorbed.iter())
            .map(|&(synth, _)| ctx.isecs[synth as usize].p2align as u32)
            .fold(3, u32::max);
        let tail = Tail::ObjcSelrefs;
        let id = tail_section(ctx, name, flags_name, flags, p2align, tail, selrefs_size);
        ctx.objc_stubs.selrefs = Some(id);
        let tail_off = ctx.output_section(id).tail_off;
        for i in 0..ctx.objc_stubs.absorbed.len() {
            let (synth, slot) = ctx.objc_stubs.absorbed[i];
            let isec = &mut ctx.isecs[synth as usize];
            isec.set_output_section(ChunkId::Output(id));
            isec.offset = (tail_off + slot as u64 * 8) as u32;
        }
    }
}

/// Lays out __objc_methlist, the method lists rewritten in the relative
/// form (see convert_objc_method_lists). ld64 lays the lists out sorted
/// by their symbol's name, each 8-byte aligned; category merging also
/// retires some after their first placement. ld-prime lays out the
/// lists -move_to_ro_segment takes to another segment (see
/// symbol_moves) alike, in an __objc_methlist there.
fn lay_out_objc_method_lists<E: Target>(
    ctx: &mut Context<E>,
    text: SectionName,
    moves: &hashbrown::HashMap<u32, Move>,
) {
    if ctx.objc_methlist.lists.is_empty() {
        return;
    }
    let mut name_of: hashbrown::HashMap<u32, &'static [u8]> = hashbrown::HashMap::new();
    let syms = ctx.symbols.syms.iter().filter_map(|sym| Some((sym.name(), sym.input_section()?)));
    // (The lists category merging builds are named as extra locals.)
    for (name, isec) in syms.chain(ctx.extra_local_syms.iter().copied()) {
        let r = ctx.resolve_isec(isec as usize) as u32;
        let e = name_of.entry(r).or_insert(name);
        if name < *e {
            *e = name;
        }
    }
    let mut order: Vec<u32> = ctx.objc_methlist.lists.iter().map(|l| l.isec).collect();
    order.sort_by_key(|&isec| (name_of.get(&isec).copied().unwrap_or_default(), isec));

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
/// (its section$start/end addresses). Each is of a file of its own in
/// the option's place among the inputs, but for the first option's,
/// which comes last (see SectCreateInput). One that names an input
/// section's output section - after -rename_section and
/// -rename_segment - joins it among the other input sections in file
/// order, byte-aligned, as a subsection of the internal object; the
/// others make sections of their own, one of each name, in the
/// command-line order of the options naming them (two options'
/// interleaved, as ld-prime creates them: that orders them within a
/// segment, and orders the segments only they make), their contents in
/// file order.
/// Runs while the output sections' members are in input order.
fn place_sectcreate_inputs<E: Target>(ctx: &mut Context<E>) {
    let map = SectionMap::final_link(ctx);
    let names: Vec<SectionName> = (ctx.args.sectcreate.iter())
        .map(|sc| map.renamed(&ctx.args, (static_name(&sc.segname), static_name(&sc.sectname))))
        .collect();
    let find_output_section = |ctx: &Context<E>, (seg, sect): SectionName| {
        let pos = (ctx.output_sections.iter())
            .position(|o| o.hdr.segname == seg && o.hdr.sectname == sect);
        pos.map(|i| OutputSectionId::new(i as u32))
    };
    let find_own_section = |ctx: &Context<E>, (seg, sect): SectionName| {
        (ctx.sectcreate_sections.iter())
            .position(|s| s.hdr.segname == seg && s.hdr.sectname == sect)
    };

    // The options' own sections, empty yet.
    for &name in &names {
        if find_output_section(ctx, name).is_none() && find_own_section(ctx, name).is_none() {
            ctx.sectcreate_sections.push(SectCreateSection::new(name.0, name.1, &[], true));
        }
    }

    // The input sections, in file order.
    let n = names.len();
    let mut in_file_order: Vec<usize> = (0..n).collect();
    in_file_order.sort_by_key(|&i| crate::chunks::sectcreate::file_priority(ctx, i));
    let mut inputs: Vec<Option<SectCreateInput>> = (0..n).map(|_| None).collect();
    let mut contents: Vec<Vec<u8>> = vec![Vec::new(); ctx.sectcreate_sections.len()];
    for i in in_file_order {
        let data: &'static [u8] = match &ctx.args.sectcreate[i].path {
            Some(path) => Vec::leak(std::fs::read(path).unwrap_or_else(|e| {
                let errno = crate::error::errno_text(&e);
                fatal!("file cannot be open()ed, {errno} path={}", path.raw())
            })),
            None => &[],
        };
        let place = match find_output_section(ctx, names[i]) {
            Some(osec) => InputPlace::Isec(add_sectcreate_isec(ctx, osec, i, data)),
            None => {
                let section = find_own_section(ctx, names[i]).unwrap();
                let offset = contents[section].len() as u64;
                contents[section].extend_from_slice(data);
                InputPlace::Section { section: section as u32, offset }
            }
        };
        inputs[i] = Some(SectCreateInput { size: data.len() as u64, place });
    }
    ctx.sectcreate_inputs = inputs.into_iter().map(Option::unwrap).collect();
    for (sec, data) in ctx.sectcreate_sections.iter_mut().zip(contents) {
        sec.hdr.size = data.len() as u64;
        sec.contents = Vec::leak(data);
    }
}

/// Adds -sectcreate option `i`'s input section of `data` to output
/// section `osec`, as a subsection of the internal object: before the
/// first input subsection of a later file. Returns the subsection.
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
        file,
        shndx,
        p2align: 0,
        input_addr: 0,
        size: data.len() as u32,
        contents: if data.is_empty() { 0 } else { data.as_ptr() as usize },
        rel_offset: 0,
        nrels: 0,
        output_section: ChunkId::Output(osec).pack(),
        offset: 0,
        flags: InputSection::flags_placed(),
        replacement: crate::input_sections::NO_REPLACEMENT,
        unwind_offset: 0,
        nunwind: 0,
    });
    let priority = crate::chunks::sectcreate::file_priority(ctx, i);
    let later = |&m: &u32| {
        let file = ctx.isecs[m].file as usize;
        !ctx.is_internal(file) && ctx.objs[file].priority > priority
    };
    let members = &ctx.output_sections[osec.index()].members;
    let at = members.iter().position(later).unwrap_or(members.len());
    ctx.output_section_mut(osec).members.insert(at, id);
    id
}

/// Adds the sections the -sectcreate and -add_empty_section options
/// make (see place_sectcreate_inputs) to the image's chunks.
fn add_sectcreate_sections<E: Target>(ctx: &mut Context<E>) {
    for i in 0..ctx.sectcreate_sections.len() {
        ctx.chunks.push(ChunkId::SectCreate(i as u32));
    }
}

/// Merges the objects' __objc_imageinfo records into the image's (see
/// passes::merge_objc_flags), in the order ld-prime checks the objects
/// in (see passes::check_objc_flags, which gave the diagnostics). An
/// image no dyld loads (-static, -preload, a kext), whose Objective-C
/// no runtime sets up, gets none from ld-prime.
fn merge_objc_image_info<E: Target>(ctx: &mut Context<E>) {
    let mut objs: Vec<&crate::input_files::ObjectFile> =
        ctx.objs.iter().filter(|o| o.is_alive && o.objc_image_info.is_some()).collect();
    objs.sort_by_key(|o| o.priority);
    let flags =
        objs.iter().filter_map(|o| o.objc_image_info).reduce(crate::passes::merge_objc_flags);
    let Some(flags) = flags else { return };
    if ctx.args.without_dyld() {
        return;
    }
    ctx.objc_imageinfo.flags = flags;
    ctx.objc_imageinfo.hdr.segname = data_seg(ctx);
    ctx.objc_imageinfo.hdr.size = 8;
    ctx.chunks.push(ChunkId::ObjcImageInfo);
}

/// Lays out __eh_frame, the surviving DWARF unwind records, in input
/// order. Their offsets are needed before layout, because the
/// __unwind_info encoding embeds each FDE's offset.
fn lay_out_eh_frame<E: Target>(ctx: &mut Context<E>) {
    // FDEs of folded copies duplicate their leader's; drop them (see
    // kept_fdes), and remap the unwind records' FDE indices around the
    // removals as the dead-strip pass does (a record left pointing past
    // the shortened table crashed the encoder).
    let mut fde_map = vec![usize::MAX; ctx.fdes.len()];
    let mut kept_fdes = Vec::new();
    let fdes = std::mem::take(&mut ctx.fdes);
    let keep = kept_fdes_of(ctx, &fdes);
    for (i, (fde, keep)) in fdes.into_iter().zip(keep).enumerate() {
        if keep {
            fde_map[i] = kept_fdes.len();
            kept_fdes.push(fde);
        }
    }
    ctx.fdes = kept_fdes;
    let map = &fde_map;
    ctx.unwind_records.retain_mut(|rec| {
        if rec.fde_idx == crate::input_files::UNWIND_NONE {
            return true;
        }
        let mapped = map[rec.fde_idx as usize];
        if mapped == usize::MAX {
            // A folded copy's record; its leader has its own.
            return false;
        }
        rec.fde_idx = mapped as u32;
        true
    });
    share_folded_fdes(ctx);

    for fde in &ctx.fdes {
        ctx.cies[fde.cie as usize].is_alive = true;
    }
    for i in 0..ctx.cies.len() {
        if ctx.keeps_lone_cie(&ctx.cies[i]) {
            ctx.cies[i].is_alive = true;
        }
    }
    if ctx.fdes.is_empty() && !ctx.cies.iter().any(|cie| cie.is_alive) {
        return;
    }
    ctx.eh_frame.hdr.flags = output_section_flags(b"__TEXT", b"__eh_frame", 0, true, false);
    ctx.eh_frame.hdr.size = assign_eh_frame_offsets(ctx) as u64;
    ctx.chunks.push(ChunkId::EhFrame);
}

/// Which of `fdes` ld-prime keeps: those of the functions -deduplicate
/// did not fold, and of a folded one, only if it folded into one of its
/// own object with its name (two copies of one local function that a -r
/// link put together), as if the FDE were the other's, which it then
/// covers, keeping the copy's unwind record with it (see
/// share_folded_fdes). The names cost a pass over the object's symbols,
/// made once for all its folded functions.
fn kept_fdes_of<E: Target>(ctx: &Context<E>, fdes: &[crate::input_files::Fde]) -> Vec<bool> {
    use crate::input_sections::NO_REPLACEMENT;
    let mut keep: Vec<bool> =
        fdes.iter().map(|fde| ctx.isecs[fde.isec as usize].replacement == NO_REPLACEMENT).collect();
    // The folded functions whose leaders are of their own objects, by
    // object: (object, FDE index, function, leader).
    let mut own: Vec<(u32, usize, usize, usize)> = (fdes.iter().enumerate())
        .filter_map(|(i, fde)| {
            let isec = fde.isec as usize;
            let leader = ctx.isecs[isec].replacement as usize;
            let file = ctx.isecs[isec].file;
            (!keep[i]
                && crate::chunks::symtab::is_coalesced_away(ctx, isec)
                && ctx.isecs[leader].file == file)
                .then_some((file, i, isec, leader))
        })
        .collect();
    own.sort_unstable();
    let named_alike: Vec<usize> = own
        .par_chunk_by(|a, b| a.0 == b.0)
        .flat_map_iter(|run| {
            let ids: Vec<usize> =
                run.iter().flat_map(|&(_, _, isec, leader)| [isec, leader]).collect();
            let names = ctx.subsec_labels(run[0].0 as usize, &ids);
            let same = |pair: &[Option<&[u8]>]| pair[0].is_some() && pair[0] == pair[1];
            let named = run.iter().zip(names.chunks(2)).filter(|(_, pair)| same(pair));
            named.map(|(e, _)| e.1).collect::<Vec<_>>()
        })
        .collect();
    for i in named_alike {
        keep[i] = true;
    }
    keep
}

/// Points the unwind records of a function and of the copies folded
/// into it that kept their FDEs (see kept_fdes_of) at the last of
/// those FDEs, as ld-prime does: __unwind_info lists one entry for each
/// at the function's address, all of them with that FDE.
fn share_folded_fdes<E: Target>(ctx: &mut Context<E>) {
    use crate::input_sections::NO_REPLACEMENT;
    let key = |rec: &crate::input_files::UnwindRecord| {
        (ctx.resolve_isec(rec.isec as usize), rec.input_offset)
    };
    let mut last: hashbrown::HashMap<(usize, u32), u32> = (ctx.unwind_records.iter())
        .filter(|rec| {
            rec.fde().is_some() && ctx.isecs[rec.isec as usize].replacement != NO_REPLACEMENT
        })
        .map(|rec| (key(rec), 0))
        .collect();
    if last.is_empty() {
        return;
    }
    for rec in ctx.unwind_records.iter().filter(|rec| rec.fde().is_some()) {
        if let Some(fde) = last.get_mut(&key(rec)) {
            *fde = (*fde).max(rec.fde_idx);
        }
    }
    let shared: Vec<Option<u32>> =
        ctx.unwind_records.iter().map(|rec| last.get(&key(rec)).copied()).collect();
    for (rec, fde) in ctx.unwind_records.iter_mut().zip(shared) {
        if let Some(fde) = fde.filter(|_| rec.fde().is_some()) {
            rec.fde_idx = fde;
        }
    }
}

/// Gives the live CIEs and the FDEs their offsets in __eh_frame and
/// returns its size. ld-prime lays the records out as the inputs have
/// them: object by object, the CIEs and FDEs of each in the order of
/// its __eh_frame (which both lists keep).
fn assign_eh_frame_offsets<E: Target>(ctx: &mut Context<E>) -> u32 {
    debug_assert!(ctx.cies.is_sorted_by_key(|cie| (cie.obj, cie.input_addr)));
    debug_assert!(ctx.fdes.is_sorted_by_key(|fde| (fde.obj, fde.input_addr)));
    let mut off = 0;
    let mut fdes = ctx.fdes.iter_mut().peekable();
    for cie in ctx.cies.iter_mut().filter(|cie| cie.is_alive) {
        let before_cie = |fde: &&mut crate::input_files::Fde| {
            (fde.obj, fde.input_addr) < (cie.obj, cie.input_addr)
        };
        while let Some(fde) = fdes.next_if(before_cie) {
            fde.output_offset = off;
            off += fde.data.len() as u32;
        }
        cie.output_offset = off;
        off += cie.data.len() as u32;
    }
    for fde in fdes {
        fde.output_offset = off;
        off += fde.data.len() as u32;
    }
    off
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
        isec.is_alive()
            && isec.replacement == crate::input_sections::NO_REPLACEMENT
            && rec.fde().is_some_and(|fde| ctx.fdes[fde].output_offset > MAX_FDE_OFFSET)
    });
    if out_of_reach {
        crate::warn!(
            "__eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected"
        );
    }
}

/// Sorts the chunks into file order: the standard segment order, and
/// section ranks within a segment. Sections of one rank follow the
/// order their first input section was seen in (see
/// section_first_seen), as ld-prime lays them out. Segment ranks honor
/// -segment_order, then the standard order; segments stay together,
/// and __LINKEDIT is always last.
fn sort_chunks<E: Target>(ctx: &mut Context<E>, lto_ranks: Option<&[u32]>) {
    let section_first_seen = section_first_seen(ctx, lto_ranks);
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
        let sect_rank = match id {
            ChunkId::MachHeader => 0,
            // After the input sections, like __unwind_info, which a
            // -static image has none of.
            ChunkId::ChainStarts => 99,
            ChunkId::UnwindInfo => 100,
            ChunkId::EhFrame => 101,
            ChunkId::CodeSignature => u32::MAX,
            _ => late_text_rank(ctx, hdr).unwrap_or_else(|| {
                1 + output_section_rank(rank_name(ctx, id), hdr.flags, static_link)
            }),
        };
        // ld-prime reads -sectcreate's and -add_empty_section's
        // contents as inputs, and makes its own content (such as the
        // selector names of the objc_msgSend$ stubs, or the lazy
        // binder's __dyld_private word) after all inputs: a section of
        // only the linker's content follows the options' sections.
        let seen = match id {
            ChunkId::Output(osec) => section_first_seen[osec.index()],
            ChunkId::SectCreate(_) => u64::MAX - 1,
            _ => u64::MAX,
        };
        // Zero-fill sections go last in their segment so that they don't
        // occupy file space in the middle of it.
        (seg_rank, hdr.is_zerofill(), listed_section_rank(ctx, hdr), sect_rank, seen)
    });
    ctx.chunks = order;
}

/// The name a chunk ranks by among the sections of its segment (see
/// output_section_rank), whatever -rename_section and -rename_segment
/// made of its own: an output section's first member's (see
/// member_rank_name), the standard name of a section the linker makes,
/// or the name a section$start$ or section$end$ symbol gives one it
/// makes. -sectcreate's and -add_empty_section's sections, which
/// ld-prime adds after the inputs', rank by their type alone.
fn rank_name<E: Target>(ctx: &Context<E>, id: ChunkId) -> Option<SectionName> {
    let name: SectionName = match id {
        ChunkId::Output(osec) => return ctx.output_section(osec).rank_name,
        ChunkId::SectCreate(i) => {
            let sec = &ctx.sectcreate_sections[i as usize];
            return (!sec.from_option).then_some((sec.hdr.segname, sec.hdr.sectname));
        }
        ChunkId::Stubs => (b"__TEXT", b"__stubs"),
        ChunkId::StubHelper => (b"__TEXT", b"__stub_helper"),
        ChunkId::DelayStubs => (b"__TEXT", b"__delay_stubs"),
        ChunkId::DelayHelper => (b"__TEXT", b"__delay_helper"),
        ChunkId::LazyHelpers => (b"__TEXT", b"__lazy_helpers"),
        ChunkId::ObjcStubs => (b"__TEXT", b"__objc_stubs"),
        ChunkId::InitOffsets => (b"__TEXT", b"__init_offsets"),
        ChunkId::ObjcMethlist => (b"__TEXT", b"__objc_methlist"),
        ChunkId::LazyPtrs => (b"__DATA", b"__la_symbol_ptr"),
        ChunkId::LazyLoadGot => (b"__DATA", b"__lazy_load_got"),
        ChunkId::Got => (b"__DATA", b"__got"),
        ChunkId::WeakGot => (b"__DATA", b"__weak_got"),
        ChunkId::ObjcImageInfo => (b"__DATA", b"__objc_imageinfo"),
        _ => return None,
    };
    Some(name)
}

/// The name an output section ranks by (see output_section_rank), from
/// its first member, an input section with header `hdr` whose flags
/// follow the name `flags_name` (see output_section_for): that name -
/// which ld-prime's own moves gave, but no rename - for a section of
/// the input's __TEXT, or one of ld-prime's standard sections (see
/// is_standard_section), crt1.o's tables or a zero-fill __zerofill in
/// its __DATA. Another, such as an input's __DATA_CONST,__const, ranks
/// by its type alone.
fn member_rank_name(hdr: &MachSection, flags_name: SectionName) -> Option<SectionName> {
    let (seg, sect) = (hdr.segname(), hdr.sectname());
    let named = match seg {
        b"__TEXT" => true,
        b"__DATA" => {
            is_standard_section(seg, sect, hdr.flags)
                || matches!(flags_name.1, b"__dyld" | b"__program_vars" | b"__zerofill")
        }
        _ => false,
    };
    named.then_some(flags_name)
}

/// When ld-prime first sees each output section, by output section: at
/// the object and section ordinal of its first input section - for
/// __common, or at the first object with a common symbol if that is
/// earlier (a C++ zero-initialized global is in an input __common). An
/// object goes by its place in input order, where what LTO compiled
/// goes by the place `lto_ranks` gives it (see lto_layout_ranks).
/// mold's own subsections don't count, nor the input selector names
/// the objc_msgSend$ stubs absorb (see is_stub_selector_name).
fn section_first_seen<E: Target>(ctx: &Context<E>, lto_ranks: Option<&[u32]>) -> Vec<u64> {
    let mut first_seen: Vec<u64> = vec![u64::MAX; ctx.output_sections.len()];
    let stub_sels: hashbrown::HashSet<&[u8]> =
        ctx.objc_stubs.symbols.iter().map(|&(_, sel)| sel).collect();
    for (i, isec) in ctx.isecs.iter().enumerate() {
        if ctx.is_internal(isec.file as usize) || is_stub_selector_name(ctx, isec, &stub_sels) {
            continue;
        }
        // A copy merged into another input's (a literal, the losing
        // copy of a weak definition) places nothing: ld-prime places a
        // section by the subsections it keeps. One a synthesized record
        // took over counts where it was.
        let kept = ctx.resolve_isec(i);
        if kept != i && !ctx.is_internal(ctx.isecs[kept].file as usize) {
            continue;
        }
        let Some(ChunkId::Output(id)) = ctx.isecs[kept].output_section() else {
            continue;
        };
        let place = lto_ranks.map_or(isec.file, |ranks| ranks[i]);
        let key = ((place as u64) << 32) | isec.shndx as u64;
        let slot = &mut first_seen[id.index()];
        *slot = (*slot).min(key);
    }
    if let Some(obj) = ctx.common_first_obj {
        let place = if lto_ranks.is_some() { ctx.objs[obj as usize].priority } else { obj };
        for (i, osec) in ctx.output_sections.iter().enumerate() {
            if osec.hdr.segname == b"__DATA" && osec.hdr.sectname == b"__common" {
                first_seen[i] = first_seen[i].min(((place as u64) << 32) | u32::MAX as u64);
            }
        }
    }
    first_seen
}

/// Groups the chunks, in file order, into segments, and numbers the
/// sections: an nlist's n_sect is the 1-based ordinal of its section in
/// the load commands.
fn create_segments<E: Target>(ctx: &mut Context<E>) {
    let mut segments = Vec::new();
    if ctx.args.pagezero_size > 0 {
        segments.push(OutputSegment::new(b"__PAGEZERO"));
    }
    let mut n_sect = 1u8;
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
            hdr.n_sect = n_sect;
            n_sect = n_sect.wrapping_add(1);
        }
    }
    ctx.segments = segments;
    // A chunk that joined a section of its name is part of it.
    for i in 0..ctx.chunks.len() {
        let hdr = ctx.chunk_header(ctx.chunks[i]);
        if let Some((joined, _)) = hdr.joined {
            let n_sect = hdr.n_sect;
            ctx.chunk_header_mut(joined).n_sect = n_sect;
        }
    }
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

/// ld-prime makes one output section of each name, whatever made its
/// contents: a section the linker synthesizes (the stubs, the GOT, the
/// lazy pointers, the Objective-C stubs, image info and method lists,
/// ...) or fills with records of its own (__dyld_private's __data) that
/// has, its renames applied, the name of an input section's output
/// section - an input __DATA_CONST,__got, or one -rename_section gives
/// the name - or of a -sectcreate or -add_empty_section section joins
/// that section. The inputs' contents come first, the options' after
/// them (as files in the options' places among the inputs, see
/// place_sectcreate_inputs), the linker's last (see content_rank); and
/// the section keeps the first contents' flags. So the GOT or the stubs
/// in a regular section are no longer typed as such: the indirect
/// symbol table leaves their slots out, and the stubs lose their stub
/// size. (ld-prime fails on a zero-fill section joined so, and on two
/// synthesized sections of one name, which keep sections of their own
/// here, as do __unwind_info and __chain_starts, which only layout
/// sizes.)
fn merge_same_name_sections<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable {
        return;
    }
    let mut i = 0;
    while i < ctx.chunks.len() {
        let id = ctx.chunks[i];
        let hdr = ctx.chunk_header(id);
        let host = content_rank(ctx, id).and_then(|rank| {
            (ctx.chunks.iter().copied())
                .filter(|&c| {
                    let h = ctx.chunk_header(c);
                    h.segname == hdr.segname
                        && h.sectname == hdr.sectname
                        && h.joined.is_none()
                        && !h.is_zerofill()
                        && !hdr.is_zerofill()
                })
                .filter_map(|c| Some((content_rank(ctx, c)?, c)))
                .filter(|&(r, _)| r < rank)
                .min_by_key(|&(r, _)| r)
        });
        let Some((_, host)) = host else {
            i += 1;
            continue;
        };
        let (size, p2align) = (hdr.size, hdr.p2align);
        ctx.chunks.remove(i);
        let hdr = ctx.chunk_header_mut(host);
        let off = align_to(hdr.size, 1 << p2align);
        hdr.joined = Some((id, off));
        hdr.size = off + size;
        hdr.p2align = hdr.p2align.max(p2align);
        if hdr.flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
            add_merged_stubs(ctx, id);
        }
    }
}

/// Where the contents of a section come in a section of its name that
/// others share (see merge_same_name_sections): those of input files
/// first, then -sectcreate's and -add_empty_section's, then the
/// records the linker makes in an output section no input has
/// contents in, then a section it synthesizes. None for a chunk that
/// joins no other: no section, or one only layout sizes.
fn content_rank<E: Target>(ctx: &Context<E>, id: ChunkId) -> Option<u8> {
    match id {
        ChunkId::Output(osec) => {
            let members = &ctx.output_section(osec).members;
            let from_input = |&m: &u32| !ctx.is_internal(ctx.isecs[m as usize].file as usize);
            Some(if members.iter().any(from_input) { 0 } else { 2 })
        }
        ChunkId::SectCreate(_) => Some(1),
        ChunkId::MachHeader | ChunkId::UnwindInfo | ChunkId::ChainStarts => None,
        _ if !ctx.chunk_header(id).is_sect => None,
        _ => Some(3),
    }
}

/// Stubs in a code section are code there, which ld-prime gives an
/// __unwind_info entry per stub as it does any code without unwind
/// information (see chunks::unwind_info::bare_code_records): a
/// subsection stands for them, at the place of the stubs' chunk.
fn add_merged_stubs<E: Target>(ctx: &mut Context<E>, id: ChunkId) {
    let stub_size = match id {
        ChunkId::Stubs => E::STUB_SIZE,
        ChunkId::ObjcStubs => ctx.objc_stub_size(),
        ChunkId::DelayStubs => E::DELAY_STUB_SIZE,
        _ => return,
    };
    let hdr = ctx.chunk_header(id);
    let size = hdr.size;
    let sect = MachSection {
        segname: bytes_to_name(hdr.segname),
        sectname: bytes_to_name(hdr.sectname),
        flags: S_ATTR_PURE_INSTRUCTIONS,
        size,
        ..Default::default()
    };
    let sect = ctx.add_synthetic_section(sect);
    let size = size as u32;
    let isec = crate::objc::add_slot_stand_in(ctx, sect);
    ctx.isecs[isec as usize].size = size;
    ctx.isecs[isec as usize].offset = 0;
    ctx.isecs[isec as usize].set_output_section(id);
    ctx.unwind_info.merged_stubs.push((isec, stub_size));
}

/// Resolves each section$start$/section$end$ and segment$start$/
/// segment$end$ symbol to the output section or segment it names, and
/// creates the sections nothing else does. ld-prime renames the name
/// as it does an input section's - __DATA,__const becomes
/// __DATA_CONST,__const, and -rename_section and -rename_segment
/// apply - or as its own section's (see SectionMap::boundary_name),
/// but merges and drops nothing: section$start$__TEXT$__literal8
/// names an empty __literal8 of its own.
fn add_boundary_sections<E: Target>(ctx: &mut Context<E>) {
    let map = SectionMap::final_link(ctx);
    for i in 0..ctx.boundary_syms.len() {
        let (_, _, seg, sect) = ctx.boundary_syms[i];
        let Some(sect) = sect else {
            ctx.boundary_syms[i].2 = renamed_segment(&ctx.args, seg);
            continue;
        };
        let flags = boundary_section_flags(seg, sect);
        let name = map.boundary_name(map.zero_fill_name((seg, sect), flags));
        let (seg, sect) = map.renamed(&ctx.args, name);
        ctx.boundary_syms[i].2 = seg;
        ctx.boundary_syms[i].3 = Some(sect);
        if !ctx.chunks.iter().any(|&id| {
            let hdr = ctx.chunk_header(id);
            hdr.is_sect && hdr.segname == seg && hdr.sectname == sect
        }) {
            let mut sec = SectCreateSection::new(seg, sect, &[], false);
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

/// The flags ld-prime gives a section only a section$start$ or
/// section$end$ symbol makes, by the name the symbol gives (before any
/// move or rename): a standard section's, though the initializer and
/// terminator lists are plain data then, and none for another name.
fn boundary_section_flags(segname: &[u8], sectname: &[u8]) -> u32 {
    match (segname, sectname) {
        (b"__DATA", b"__mod_init_func" | b"__mod_term_func") => S_REGULAR,
        // -merge_zero_fill_sections's section, whether or not given.
        (b"__DATA", b"__zerofill") => S_ZEROFILL,
        _ => standard_section_flags(segname, sectname).unwrap_or(S_REGULAR),
    }
}

/// Sizes the stubs, the lazy-binding helper and pointers, the
/// lazy-load helpers and slots, and the GOT (with __weak_got split off,
/// see GotSection), and adds the ones in use to the output.
fn add_stub_and_got_chunks<E: Target>(ctx: &mut Context<E>) {
    if !ctx.stubs.symbols.is_empty() {
        if ctx.args.text_exec {
            ctx.stubs.hdr.segname = b"__TEXT_EXEC";
        }
        ctx.stubs.hdr.reserved2 = E::STUB_SIZE as u32;
        ctx.stubs.hdr.size = ctx.stubs.symbols.len() as u64 * E::STUB_SIZE;
        // ld-prime's x86-64 stubs are byte-aligned when all of them go
        // through the lazy-binding helper and 2-byte aligned as soon as
        // one doesn't (chained fixups, -bind_at_load, a weak-lookup
        // stub): each stub has its own alignment and the section takes
        // the largest. arm64's are instruction-aligned.
        if E::CPUTYPE == crate::macho::CPU_TYPE_X86_64 {
            let lazy = ctx.args.lazy_binding
                && ctx.stubs.symbols.iter().all(|&id| !ctx.binds_weak_lookup(id));
            ctx.stubs.hdr.p2align = if lazy { 0 } else { 1 };
        }
        ctx.chunks.push(ChunkId::Stubs);
    }
    // (A stub bound by weak lookup goes through the GOT; only lazily
    // bound stubs need the helper and lazy pointers.)
    if !ctx.stubs.lazy.is_empty() {
        ctx.stub_helper.hdr.size = ctx.stub_helper_header_size()
            + ctx.stubs.lazy.len() as u64 * E::STUB_HELPER_ENTRY_SIZE
            - ctx.stub_helper_entry_padding();
        ctx.chunks.push(ChunkId::StubHelper);
        // In the shared region, dyld binds them all at load, and the
        // section joins the read-only data.
        if ctx.args.shared_region {
            ctx.lazy_ptrs.hdr.segname = data_seg(ctx);
        }
        ctx.lazy_ptrs.hdr.size = ctx.stubs.lazy.len() as u64 * 8;
        ctx.chunks.push(ChunkId::LazyPtrs);
    }

    // The delay-init stubs and helpers.
    let delay = &mut ctx.delay_init;
    if !delay.stubs.is_empty() {
        delay.stubs_hdr.segname = ctx.stubs.hdr.segname;
        delay.stubs_hdr.p2align = E::DELAY_P2ALIGN;
        delay.stubs_hdr.size = delay.stubs.len() as u64 * E::DELAY_STUB_SIZE;
        ctx.chunks.push(ChunkId::DelayStubs);
    }
    if let Some(last) = delay.dlopens.last() {
        delay.helper_hdr.segname = ctx.stubs.hdr.segname;
        delay.helper_hdr.p2align = E::DELAY_P2ALIGN;
        delay.helper_hdr.size = (last.offset + E::DLOPEN_HELPER_SIZE) as u64;
        ctx.chunks.push(ChunkId::DelayHelper);
    }

    // The lazy-load helpers, and their slots: read-only data in the
    // shared region, as its lazy pointers are.
    if !ctx.lazy_helpers.helpers.is_empty() {
        let last = ctx.lazy_helpers.helpers.last().unwrap();
        let size = last.offset + E::lazy_helper_size(last.kind);
        let hdr = &mut ctx.lazy_helpers.hdr;
        hdr.segname = ctx.stubs.hdr.segname;
        hdr.p2align = E::LAZY_HELPERS_P2ALIGN;
        hdr.size = size as u64;
        ctx.chunks.push(ChunkId::LazyHelpers);
    }
    if !ctx.lazy_load_got.slots.is_empty() {
        if ctx.args.shared_region {
            ctx.lazy_load_got.hdr.segname = data_seg(ctx);
        }
        ctx.lazy_load_got.hdr.size = ctx.lazy_load_got.slots.len() as u64 * 8;
        ctx.chunks.push(ChunkId::LazyLoadGot);
    }

    let got = &mut ctx.got;
    let weak = got.got_syms.len() - got.weak_start;
    let slots = got.weak_start + got.input_slots.len();
    got.hdr.size = slots as u64 * 8;
    got.weak_hdr.size = weak as u64 * 8;
    for (j, &i) in got.input_slots.iter().enumerate() {
        ctx.isecs[i as usize].offset = ((got.weak_start + j) * 8) as u32;
        ctx.isecs[i as usize].set_output_section(ChunkId::Got);
    }
    let seg = data_seg(ctx);
    // A kext's are plain data to ld-prime (indexed into the indirect
    // symbol table all the same), and so are a -static image's, but for
    // a PIE's - one that has an indirect symbol table.
    let plain = ctx.args.is_kext() || (ctx.args.static_link && !ctx.args.pie);
    let flags = if plain { S_REGULAR } else { S_NON_LAZY_SYMBOL_POINTERS };
    for (id, len) in [(ChunkId::Got, slots), (ChunkId::WeakGot, weak)] {
        if len > 0 {
            let hdr = ctx.chunk_header_mut(id);
            hdr.segname = seg;
            hdr.flags = flags;
            ctx.chunks.push(id);
        }
    }
    for i in 0..ctx.got.stand_ins.len() {
        let (slot, id) = ctx.got.stand_ins[i];
        let (chunk, off) = ctx.got.slot_place(ctx.sym_aux(id).got_idx as usize);
        ctx.isecs[slot as usize].offset = off as u32;
        ctx.isecs[slot as usize].set_output_section(chunk);
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

/// Settles each output section that renames fill with both zero-fill
/// and file-backed input sections - bits 0 and 1 of `fill_kinds`, by
/// output section - as ld-prime does, with a warning. The section
/// takes the type of its first member in ld-prime's order: the
/// objects' in input order, a common symbol's __common after its
/// object's own sections. A zero-fill section drops the contents of
/// the others.
fn resolve_zerofill_conflicts<E: Target>(ctx: &mut Context<E>, fill_kinds: &[u8]) {
    if !fill_kinds.contains(&3) {
        return;
    }
    let owners = common_owners(ctx);
    for (i, &kinds) in fill_kinds.iter().enumerate() {
        if kinds == 3 {
            resolve_zerofill_conflict(ctx, OutputSectionId::new(i as u32), &owners);
        }
    }
}

fn resolve_zerofill_conflict<E: Target>(
    ctx: &mut Context<E>,
    id: OutputSectionId,
    common_owners: &hashbrown::HashMap<u32, u32>,
) {
    // The first file with a zero-fill member, whether that is a
    // common symbol, and the files with file-backed ones. A common
    // symbol's subsection, which mold makes in its internal object,
    // counts as its owner's; the others mold makes come from no file.
    let mut defined_in: Option<(u32, bool)> = None;
    let mut missing_in = Vec::new();
    for &member in &ctx.output_section(id).members {
        let file = ctx.isecs[member as usize].file;
        let file = if !ctx.is_internal(file as usize) {
            (file, false)
        } else if let Some(&owner) = common_owners.get(&member) {
            (owner, true)
        } else {
            continue;
        };
        if ctx.hdr_of(&ctx.isecs[member as usize]).is_zerofill() {
            defined_in = Some(defined_in.map_or(file, |first| first.min(file)));
        } else {
            missing_in.push(file.0);
        }
    }
    let Some((defined_in, is_common)) = defined_in else {
        return;
    };
    // mold makes common symbols' subsections last; one that comes
    // first to ld-prime makes the section zero-fill.
    if is_common && missing_in.iter().all(|&file| defined_in < file) {
        let hdr = &mut ctx.output_section_mut(id).hdr;
        hdr.flags = (hdr.flags & !SECTION_TYPE) | S_ZEROFILL;
    }
    missing_in.sort_unstable_by(|a, b| b.cmp(a));
    missing_in.dedup();
    let osec = ctx.output_section(id);
    let name = |file: u32| resolved_file_name(ctx.objs[file as usize].mf);
    let mut msg = crate::error::render(format_args!(
        "section {},{} has a conflicting zerofill flag defined in {} but missing in:",
        raw(osec.hdr.segname),
        raw(osec.hdr.sectname),
        name(defined_in)
    ));
    for file in missing_in {
        msg.extend(crate::error::render(format_args!("\n  {}", name(file))));
    }
    crate::warn!("{}", raw(&msg));
}

/// The object each common symbol's subsection stands for the tentative
/// definition of, by subsection: the one declaring the largest size,
/// the first of equals, as in ld-prime.
pub(crate) fn common_owners<E: Target>(ctx: &Context<E>) -> hashbrown::HashMap<u32, u32> {
    let mut decls: hashbrown::HashMap<crate::symbol::SymbolId, (u64, u32)> =
        hashbrown::HashMap::new();
    for (i, obj) in ctx.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive) {
        let r = obj.global_range();
        for (nlist, &sym) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if !nlist.is_stab() && nlist.n_type() == N_UNDF && nlist.is_common() {
                let decl = decls.entry(sym).or_insert((nlist.n_value, i as u32));
                if nlist.n_value > decl.0 {
                    *decl = (nlist.n_value, i as u32);
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
                let errno = crate::error::errno_text(&e);
                crate::warn!("order file '{}' could not be opened, {errno}", path.raw());
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
