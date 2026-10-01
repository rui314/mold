//! The output sections: which output section each input section goes
//! to, under what name and with what flags (ld-prime's rules for both),
//! the sections the linker synthesizes, and the order of the sections
//! and segments in the file.

use std::os::unix::ffi::OsStrExt;

use rayon::prelude::*;

use crate::chunks::sectcreate::SectCreateSection;
use crate::chunks::{
    self, ChunkHeader, ChunkId, OutputSection, OutputSectionId, OutputSegment, Tail,
};
use crate::context::Context;
use crate::error;
use crate::fatal;
use crate::input_files::FileId;
use crate::input_sections::InputSection;
use crate::macho::*;
use crate::objc::{DataBlob, cstring_of};
use crate::passes::{is_class_or_protocol_ref_name, resolved_file_name};
use crate::target::Target;
use crate::util::{align_to, path_bytes};

/// Where a section sits within its segment in a final image, as
/// ld-prime 27037 orders them; sections of one rank keep input order.
/// __text leads __TEXT, other code sections follow in input order, then
/// the synthesized code and method lists; dyld's tables lead
/// __DATA_CONST, then the read-only ObjC lists in a fixed order; the
/// ObjC runtime data leads __DATA in the order the compiler emits it,
/// and __data follows in input order among the unknown sections.
fn output_section_rank(segname: &str, sectname: &str, flags: u32) -> u32 {
    match (segname, sectname) {
        ("__TEXT", "__text") => 0,
        ("__TEXT", "__stubs") => 2,
        ("__TEXT", "__stub_helper") => 3,
        ("__TEXT", "__lazy_helpers") => 4,
        ("__TEXT", "__objc_stubs") => 5,
        ("__TEXT", "__init_offsets") => 6,
        ("__TEXT", "__objc_methlist") => 7,
        ("__TEXT", _) if flags & S_ATTR_PURE_INSTRUCTIONS != 0 => 1,
        ("__TEXT", _) => 10,
        ("__DATA_CONST", "__mod_init_func") => 1,
        ("__DATA_CONST", "__mod_term_func") => 2,
        ("__DATA_CONST", "__const") => 3,
        ("__DATA_CONST", "__cfstring") => 4,
        ("__DATA_CONST", "__objc_classlist") => 5,
        ("__DATA_CONST", "__objc_nlclslist") => 6,
        ("__DATA_CONST", "__objc_catlist") => 7,
        ("__DATA_CONST", "__objc_catlist2") => 8,
        ("__DATA_CONST", "__objc_nlcatlist") => 9,
        ("__DATA_CONST", "__objc_protolist") => 10,
        ("__DATA_CONST", "__objc_imageinfo") => 11,
        ("__DATA_CONST", "__objc_protorefs") => 12,
        ("__DATA_CONST", "__objc_classrefs") => 13,
        ("__DATA_CONST", "__objc_superrefs") => 14,
        // The GOT closes __DATA_CONST, after every input-derived
        // section (ld-prime: __cfstring, __objc_classlist,
        // __objc_imageinfo, then __got). In the shared region the lazy
        // pointers lead it (the lazy-load slots after them), and the
        // class data, __weak_got and the selector references come
        // before __got.
        ("__DATA_CONST", "__la_symbol_ptr" | "__lazy_load_got") => 0,
        ("__DATA_CONST", "__objc_const") => 21,
        ("__DATA_CONST", "__weak_got") => 22,
        ("__DATA_CONST", "__objc_selrefs") => 23,
        ("__DATA_CONST", "__got") => 25,
        ("__DATA_CONST", _) => 20,
        // Without __DATA_CONST (-no_data_const, an x86-64 kext),
        // ld-prime's __DATA starts with the lazy pointers and the
        // initializer and terminator lists, and the GOT follows the
        // input sections.
        ("__DATA", "__la_symbol_ptr") => 0,
        ("__DATA", "__mod_init_func" | "__mod_term_func") => 1,
        ("__DATA", "__got") => 25,
        ("__DATA", "__objc_const") => 2,
        ("__DATA", "__objc_selrefs") => 3,
        ("__DATA", "__objc_protorefs") => 4,
        ("__DATA", "__objc_classrefs") => 5,
        ("__DATA", "__objc_superrefs") => 6,
        ("__DATA", "__objc_ivar") => 7,
        ("__DATA", "__objc_data") => 8,
        ("__DATA", "__lazy_load_got") => 9,
        // The thread-local initialization image must be contiguous: its
        // initial values (__thread_data) last among file-backed __DATA
        // sections, after the variables' descriptors (__thread_vars),
        // and its zero fill (__thread_bss) first among zero-fill ones
        // (zero-fill sections sort after all file-backed ones).
        // ld-prime goes by the section types, whatever the names.
        ("__DATA", _) if flags & SECTION_TYPE == S_THREAD_LOCAL_VARIABLES => 30,
        ("__DATA", _) if flags & SECTION_TYPE == S_THREAD_LOCAL_REGULAR => 31,
        ("__DATA", _) if flags & SECTION_TYPE == S_THREAD_LOCAL_ZEROFILL => 0,
        // __bss and __common in first-seen order: the synthesized
        // __common counts from the first object with a common symbol.
        ("__DATA", "__bss") => 3,
        ("__DATA", "__common") => 3,
        _ => 10,
    }
}

/// Where ld-prime moves a __TEXT section of an image bound for the
/// shared region: the stubs and the Objective-C names, which the shared
/// cache builder bypasses and uniques, come after __unwind_info (100)
/// and __eh_frame (101), the names in a fixed order.
fn shared_region_text_rank<E: Target>(
    ctx: &Context<E>,
    hdr: &crate::chunks::ChunkHeader,
) -> Option<u32> {
    if !ctx.args.shared_region || hdr.segname != "__TEXT" {
        return None;
    }
    match hdr.sectname.as_str() {
        "__objc_stubs" => Some(102),
        "__stubs" => Some(103),
        "__objc_classname" => Some(104),
        "__objc_methname" => Some(105),
        "__objc_methtype" => Some(106),
        _ => None,
    }
}

/// The segment for read-only-after-fixup data: __DATA_CONST unless
/// -no_data_const.
pub(crate) fn data_seg<E: Target>(ctx: &Context<E>) -> &'static str {
    if ctx.args.data_const { "__DATA_CONST" } else { "__DATA" }
}

/// Sections a final link places in __DATA_CONST: data that needs no
/// writes after dyld's fixups. ld-prime's list - signed pointers
/// (__auth_ptr), CF and ObjC constant objects, the ObjC lists and the
/// initializer lists - but for the ones only a condition moves (see
/// SectionMap::const_name). A section not on it, such as
/// __objc_boolobj, stays in __DATA.
const DATA_CONST_SECTIONS: &[&str] = &[
    "__auth_ptr",
    "__cfstring",
    "__const",
    "__const_cfobj2",
    "__got",
    "__mod_init_func",
    "__mod_term_func",
    "__objc_arraydata",
    "__objc_arrayobj",
    "__objc_dateobj",
    "__objc_dictobj",
    "__objc_doubleobj",
    "__objc_floatobj",
    "__objc_intobj",
    "__objc_catlist",
    "__objc_catlist2",
    "__objc_classlist",
    "__objc_imageinfo",
    "__objc_nlcatlist",
    "__objc_nlclslist",
    "__objc_protolist",
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
/// with -no_data_const.
fn interpose_is_const<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.platform == crate::macho::PLATFORM_MACOS
        && ctx.args.platform_minos >= crate::macho::encode_version(15, 0, 0)
        && !ctx.args.without_dyld()
}

/// An output section's name: (segment, section).
type SectionName = (&'static str, &'static str);

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
    segname: &str,
    sectname: &str,
    flags: u32,
) -> Option<(SectionName, SectionName)> {
    if segname == "__LLVM" {
        return None;
    }
    let name = (static_name(segname), static_name(sectname));
    if map.relocatable {
        return Some((renamed(args, name), name));
    }
    if name == ("__DATA", "__objc_clsrolist") {
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

/// The section a final link merges a __TEXT section into, like ld64:
/// __StaticInit joins __text, and the literal pools join __const.
fn merged_name(name: (&str, &str)) -> Option<SectionName> {
    match name {
        ("__TEXT", "__StaticInit") => Some(("__TEXT", "__text")),
        ("__TEXT", "__literal4" | "__literal8" | "__literal16") => Some(("__TEXT", "__const")),
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
    let (seg, sect) = name;
    let (seg, sect) = match args.rename_sections.iter().find(|(s, t, _, _)| s == seg && t == sect) {
        Some((_, _, s, t)) => (static_name(s), static_name(t)),
        None => modern_name(name),
    };
    (renamed_segment(args, seg), sect)
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
        ("__TEXT", "__textcoal_nt") => ("__TEXT", "__text"),
        ("__TEXT" | "__DATA" | "__DATA_CONST", "__const_coal") => (name.0, "__const"),
        ("__DATA" | "__DATA_DIRTY", "__datacoal_nt") => (name.0, "__data"),
        _ => name,
    }
}

/// The segment -rename_segment moves a segment's sections to.
fn renamed_segment(args: &crate::cmdline::Args, seg: &'static str) -> &'static str {
    match args.rename_segments.iter().find(|(old, _)| old == seg) {
        Some((_, new)) => static_name(new),
        None => seg,
    }
}

/// A section or segment name that lives as long as the output's
/// headers: the usual segment names are literals, and the rest are
/// leaked (the callers name each distinct section once).
fn static_name(name: &str) -> &'static str {
    match name {
        "__TEXT" => "__TEXT",
        "__DATA_CONST" => "__DATA_CONST",
        "__DATA" => "__DATA",
        _ => String::leak(name.to_string()),
    }
}

/// What decides where output_section_for puts an input section.
#[derive(Clone, Copy)]
struct SectionMap {
    relocatable: bool,
    data_const: bool,
    objc_const_refs: bool,
    const_interpose: bool,
    shared_region: bool,
    relative_methods: bool,
    text_exec: bool,
}

impl SectionMap {
    /// The name ld-prime gives an input section of a final image, with
    /// the section's `flags`, before -rename_section and
    /// -rename_segment: with -text_exec (an arm64 kext) every section
    /// of code - pure instructions, in any segment - moves into
    /// __TEXT_EXEC,__text, and data that needs no writes after fixups
    /// to __DATA_CONST - as do the non-lazy symbol pointers of a
    /// __DATA section of any name, which ld-prime knows by their type
    /// (see output_section_flags).
    fn builtin_name(self, name: SectionName, flags: u32) -> SectionName {
        if self.text_exec && flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
            return ("__TEXT_EXEC", "__text");
        }
        if !is_standard_section(name.0, name.1, flags) {
            if name.0 == "__DATA"
                && flags & SECTION_TYPE == S_NON_LAZY_SYMBOL_POINTERS
                && self.data_const
            {
                return ("__DATA_CONST", name.1);
            }
            return name;
        }
        self.const_name(name)
    }

    /// A standard __DATA section's name in a final image when it needs
    /// no writes after dyld's fixups: the same section in __DATA_CONST,
    /// unless -no_data_const - in the shared region, where dyld fixes
    /// them up for good, the selector references and the Objective-C
    /// runtime's class data too. ld-prime treats this move as a
    /// renaming, which boundary symbols follow as well (unlike
    /// -text_exec's: section$start$__TEXT$__text stays in __TEXT).
    fn const_name(self, name: SectionName) -> SectionName {
        let (seg, sect) = name;
        let is_const = match sect {
            "__objc_classrefs" | "__objc_protorefs" | "__objc_superrefs" => self.objc_const_refs,
            "__objc_selrefs" => self.shared_region,
            // Unless it holds absolute method lists, which the runtime
            // sorts in place.
            "__objc_const" => self.shared_region && self.relative_methods,
            _ => DATA_CONST_SECTIONS.contains(&sect),
        };
        if seg == "__DATA" && self.data_const && is_const { ("__DATA_CONST", sect) } else { name }
    }

    /// The section a section$start$ or section$end$ symbol names: the
    /// one an input section of that name lands in - or, for a pointer
    /// section only the linker makes, where ld-prime puts it: its GOTs
    /// in __DATA_CONST and, in the shared region, its lazy pointers
    /// too. (An input section of one of those names is data like any
    /// other to ld-prime, which rejects one typed as pointers.)
    fn boundary_name(self, name: SectionName) -> SectionName {
        let is_const = match name {
            ("__DATA", "__auth_got" | "__weak_got" | "__weak_auth_got") => true,
            ("__DATA", "__la_symbol_ptr" | "__lazy_load_got") => self.shared_region,
            _ => false,
        };
        if self.data_const && is_const { ("__DATA_CONST", name.1) } else { self.const_name(name) }
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
        let is_renamed = args.rename_sections.iter().any(|(s, t, _, _)| s == name.0 && t == name.1);
        if name == ("__DATA", "__interpose") && self.const_interpose && !is_renamed {
            return (renamed_segment(args, "__DATA_CONST"), name.1);
        }
        renamed(args, name)
    }

    fn new<E: Target>(ctx: &Context<E>) -> Self {
        Self {
            relocatable: ctx.args.relocatable,
            data_const: ctx.args.data_const,
            objc_const_refs: objc_refs_are_const(ctx),
            const_interpose: interpose_is_const(ctx),
            shared_region: ctx.args.shared_region,
            relative_methods: ctx.args.objc_relative_method_lists,
            text_exec: ctx.args.text_exec,
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
    segname: &str,
    sectname: &str,
    input: u32,
    standard: bool,
    relocatable: bool,
) -> u32 {
    if segname == "__TEXT" && sectname == "__eh_frame" {
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
        if (segname, sectname) == ("__DATA", "__got") {
            return input & !SECTION_TYPE;
        }
        return input;
    }
    // The two reference lists the runtime may still write keep the
    // flags they came with (coalesced, no-dead-strip) while in __DATA
    // of a final image, and the protocol list its coalesced type.
    if standard && segname == "__DATA" {
        match sectname {
            "__objc_protorefs" | "__objc_superrefs" => {
                return input & (SECTION_TYPE | S_ATTR_NO_DEAD_STRIP);
            }
            "__objc_protolist" => return input & SECTION_TYPE,
            _ => {}
        }
    }
    // ld-prime knows __objc_selrefs by name: its selector references
    // stay literal pointers whatever their type - but those typed so,
    // which it makes plain data once constant (in the shared region),
    // as it does the class references.
    if standard && sectname == "__objc_selrefs" {
        return if segname == "__DATA_CONST" && input & SECTION_TYPE == S_LITERAL_POINTERS {
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
            "__objc_classlist"
                | "__objc_catlist"
                | "__objc_catlist2"
                | "__objc_nlclslist"
                | "__objc_nlcatlist"
        ) || (segname == "__DATA" && sectname == "__objc_classrefs"))
    {
        attrs |= S_ATTR_NO_DEAD_STRIP;
    }
    ty | attrs
}

/// The flags ld-prime reads a section of an input object as having,
/// which decide how the link splits the section into atoms and what it
/// makes of them - mold's canonicalize_type for a section typed by name
/// alone. __TEXT,__constructor, where GCC put the constructors of code
/// built without dyld (-static, -mkernel) with the assembler's
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
/// has_unnamed_atoms), though the output has the table's flags. Its
/// own flags otherwise.
pub(crate) fn canonical_section_flags(segname: &str, sectname: &str, flags: u32) -> u32 {
    if (segname, sectname) == ("__TEXT", "__constructor") {
        return S_MOD_INIT_FUNC_POINTERS;
    }
    let ty = flags & SECTION_TYPE;
    let is_literal =
        matches!(ty, S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS);
    if (segname, sectname) == ("__DATA", "__got") {
        return if is_literal { flags & !SECTION_TYPE } else { flags };
    }
    if !sectname.starts_with("__objc_") {
        return flags;
    }
    let Some(table) = standard_section_flags(segname, sectname) else {
        return flags;
    };
    if sectname == "__objc_selrefs" {
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
fn is_standard_section(segname: &str, sectname: &str, flags: u32) -> bool {
    let Some(table) = standard_section_flags(segname, sectname) else {
        return false;
    };
    table & SECTION_TYPE == flags & SECTION_TYPE
        || sectname.starts_with("__objc_")
        || (segname, sectname) == ("__DATA", "__got")
}

/// The flags ld-prime reads an input section as having, from its
/// canonical ones (see canonical_section_flags): those its table holds
/// for the section's name (see standard_section_flags) if the section
/// has the table's type - a __TEXT,__const or __DATA,__data an
/// assembler nop landed in is plain data again, a regular __text
/// code - and its own otherwise (a regular __cstring holds no literals
/// to merge). (A final image has no input __got left: its slots are
/// the GOT's, see fold_input_got.)
fn input_section_flags(segname: &str, sectname: &str, flags: u32) -> u32 {
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
fn standard_section_flags(segname: &str, sectname: &str) -> Option<u32> {
    let flags = match (segname, sectname) {
        (
            "__TEXT",
            "__text" | "__StaticInit" | "__stub_helper" | "__objc_stubs" | "__objc_clsstubs"
            | "__delay_stubs" | "__delay_helper" | "__lazy_helpers" | "__resolver_help",
        ) => S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS,
        (
            "__TEXT",
            "__cstring" | "__objc_classname" | "__objc_methname" | "__objc_methtype"
            | "__oslogstring",
        ) => S_CSTRING_LITERALS,
        ("__TEXT", "__literal4") => S_4BYTE_LITERALS,
        ("__TEXT", "__literal8") => S_8BYTE_LITERALS,
        ("__TEXT", "__literal16") => S_16BYTE_LITERALS,
        ("__TEXT", "__eh_frame") => output_section_flags(segname, sectname, 0, true, false),
        ("__TEXT", "__const" | "__ustring" | "__gcc_except_tab" | "__objc_methlist") => S_REGULAR,
        ("__DATA", "__got" | "__auth_got" | "__weak_got" | "__weak_auth_got") => {
            S_NON_LAZY_SYMBOL_POINTERS
        }
        ("__DATA", "__la_symbol_ptr" | "__la_resolver") => S_LAZY_SYMBOL_POINTERS,
        ("__DATA", "__mod_init_func") => S_MOD_INIT_FUNC_POINTERS,
        ("__DATA", "__mod_term_func") => S_MOD_TERM_FUNC_POINTERS,
        (
            "__DATA",
            "__objc_classlist" | "__objc_nlclslist" | "__objc_catlist" | "__objc_catlist2"
            | "__objc_nlcatlist" | "__objc_classrefs" | "__objc_superrefs" | "__objc_clsrolist",
        ) => S_ATTR_NO_DEAD_STRIP,
        ("__DATA", "__objc_protolist") => S_COALESCED,
        ("__DATA", "__objc_protorefs") => S_COALESCED | S_ATTR_NO_DEAD_STRIP,
        ("__DATA", "__objc_selrefs") => S_LITERAL_POINTERS | S_ATTR_NO_DEAD_STRIP,
        ("__DATA", "__thread_vars") => S_THREAD_LOCAL_VARIABLES,
        ("__DATA", "__thread_ptrs") => S_THREAD_LOCAL_VARIABLE_POINTERS,
        ("__DATA", "__thread_data") => S_THREAD_LOCAL_REGULAR,
        ("__DATA", "__thread_bss") => S_THREAD_LOCAL_ZEROFILL,
        ("__DATA", "__bss" | "__common") => S_ZEROFILL,
        (
            "__DATA",
            "__data" | "__const" | "__cfstring" | "__auth_ptr" | "__objc_data" | "__objc_const"
            | "__objc_ivar" | "__objc_imageinfo" | "__objc_intobj" | "__objc_floatobj"
            | "__objc_doubleobj" | "__objc_dateobj" | "__objc_dictobj" | "__objc_arrayobj"
            | "__objc_arraydata" | "__const_cfobj2",
        ) => S_REGULAR,
        // The compiler's records for the linker to encode into
        // __unwind_info, which no output carries (but a boundary
        // symbol's empty section).
        ("__LD", "__compact_unwind") => S_ATTR_DEBUG,
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
        && ctx.hdr_of(isec).sectname() == "__objc_methname"
        && stub_sels.contains(cstring_of(isec.data()))
}

/// The output section named `seg`,`sect` - created if no input made
/// one - with a synthesized `tail` of `tail_size` bytes appended after
/// its input subsections, which are placed already.
fn tail_section<E: Target>(
    ctx: &mut Context<E>,
    seg: &'static str,
    sect: &str,
    flags: u32,
    p2align: u32,
    tail: Tail,
    tail_size: u64,
) -> OutputSectionId {
    let id = match ctx
        .output_sections
        .iter()
        .position(|o| o.hdr.segname == seg && o.hdr.sectname == sect)
    {
        Some(i) => OutputSectionId::new(i as u32),
        None => {
            let mut osec = OutputSection::new(seg, sect);
            osec.hdr.flags = flags;
            let id = OutputSectionId::new(ctx.output_sections.len() as u32);
            ctx.output_sections.push(osec);
            ctx.chunks.push(ChunkId::Output(id));
            id
        }
    };
    let osec = ctx.output_section_mut(id);
    osec.hdr.p2align = osec.hdr.p2align.max(p2align);
    osec.tail = tail;
    osec.tail_off = align_to(osec.hdr.size, 1 << p2align);
    osec.hdr.size = osec.tail_off + tail_size;
    id
}

/// A record category merging rewrites in place of an input subsection,
/// such as a class's ro data, takes that subsection's position among
/// its output section's members, as ld-prime keeps
/// __OBJC_CLASS_RO_$_Foo where the input had it. Runs while the members
/// are still in input order; the other synthesized records go in the
/// section's tail.
fn place_replacing_blobs<E: Target>(ctx: &mut Context<E>) {
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
        let hdr = ctx.hdr_of(&ctx.isecs[replaced as usize]);
        let map = SectionMap::final_link(ctx);
        let out = output_section_for(&ctx.args, map, hdr.segname(), hdr.sectname(), hdr.flags);
        let Some(pos) = out.and_then(|((seg, sect), _)| {
            ctx.output_sections.iter().position(|o| o.hdr.segname == seg && o.hdr.sectname == sect)
        }) else {
            continue;
        };
        let p2align = ctx.isecs[blob as usize].p2align as u32;
        let osec = &mut ctx.output_sections[pos];
        let at = osec.members.partition_point(|&m| m < replaced);
        osec.members.insert(at, blob);
        osec.has_blobs = true;
        osec.hdr.p2align = osec.hdr.p2align.max(p2align);
        ctx.isecs[blob as usize]
            .set_output_section(ChunkId::Output(OutputSectionId::new(pos as u32)));
    }
}

/// The synthesized Objective-C records not placed among the inputs go
/// in the tail of the section they name.
fn place_tail_blobs<E: Target>(ctx: &mut Context<E>) {
    let unplaced =
        |ctx: &Context<E>, b: &DataBlob| ctx.isecs[b.isec as usize].output_section().is_none();
    let mut sects: Vec<&'static str> =
        ctx.data_blobs.iter().filter(|b| unplaced(ctx, b)).map(|b| b.sect).collect();
    sects.sort();
    sects.dedup();
    for sect in sects {
        let map = SectionMap::final_link(ctx);
        let ((seg, out), (flags_seg, flags_sect)) =
            output_section_for(&ctx.args, map, "__DATA", sect, 0).unwrap();
        let flags = output_section_flags(flags_seg, flags_sect, 0, true, false);
        // Each record at its own alignment (a pointer's, but for the
        // lazy-load flag words), the tail at the first one's; laid out
        // from where the tail will start, so that the offsets within
        // the section are aligned.
        let blobs: Vec<(u32, u64, u32)> = (ctx.data_blobs.iter())
            .filter(|b| b.sect == sect && unplaced(ctx, b))
            .map(|b| (b.isec, b.size(), ctx.isecs[b.isec as usize].p2align as u32))
            .collect();
        let first = blobs[0].2;
        let start = ctx
            .output_sections
            .iter()
            .find(|o| o.hdr.segname == seg && o.hdr.sectname == out)
            .map_or(0, |o| align_to(o.hdr.size, 1 << first));
        let mut end = start;
        let mut offs = Vec::new();
        for &(isec, size, p2align) in &blobs {
            end = align_to(end, 1 << p2align);
            offs.push((isec, end));
            end += size;
        }
        let id = tail_section(ctx, seg, out, flags, first, Tail::DataBlobs, end - start);
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
    assign_input_sections(ctx, text);
    place_replacing_blobs(ctx);

    // A final image always has a __text section, empty if no code
    // reached it (a dylib of only data; ld-prime writes one of size 0,
    // byte-aligned).
    if !ctx.args.relocatable && find_output_section(ctx, text).is_none() {
        let flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        add_output_section(ctx, text.0, text.1, flags);
    }

    set_section_alignments(ctx);
    sort_section_members(ctx);
    compute_section_sizes(ctx);

    // The sections the linker synthesizes.
    add_stub_and_got_chunks(ctx);
    if !ctx.init_offsets.init_funcs.is_empty() {
        ctx.init_offsets.hdr.size = ctx.init_offsets.init_funcs.len() as u64 * 4;
        ctx.chunks.push(ChunkId::InitOffsets);
    }
    add_objc_stubs(ctx);
    place_tail_blobs(ctx);
    lay_out_objc_method_lists(ctx);
    add_sectcreate_sections(ctx);
    merge_objc_image_info(ctx);
    if ctx.args.unwind_info() && chunks::unwind_info::is_needed(ctx) {
        ctx.chunks.push(ChunkId::UnwindInfo);
    }
    lay_out_eh_frame(ctx);
    add_linkedit_chunks(ctx);
    rename_synthetic_sections(ctx);
    add_boundary_sections(ctx);

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
/// output_section_for), creating the output sections in the order their
/// first members come, and drops the sections the link consumes. A
/// final image's sections that renames made of zero-fill and
/// file-backed members alike are then settled.
fn assign_input_sections<E: Target>(ctx: &mut Context<E>, text: SectionName) {
    let map = SectionMap::new(ctx);
    // Each input section name's output section, keyed by the raw
    // 16-byte names, so that the hot loop does no allocation and no
    // linear scans - and by its flags too, which say whether -text_exec
    // moves it and whether it is the standard section of its name (see
    // is_standard_section).
    let mut by_name: hashbrown::HashMap<([u8; 16], [u8; 16], u32), Option<OutputSectionId>> =
        hashbrown::HashMap::new();
    // Output sections by their (possibly renamed) names: several input
    // section names can land in one output section.
    let mut by_out: hashbrown::HashMap<(&'static str, &'static str), OutputSectionId> =
        hashbrown::HashMap::new();
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
    for i in 0..ctx.isecs.len() {
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
        let osec_id = if hdr_ptr == last_hdr {
            last_osec
        } else {
            let key = (hdr.segname, hdr.sectname, hdr.flags);
            let id = match by_name.get(&key) {
                Some(&id) => id,
                None => {
                    let (seg, sect) = (hdr.segname(), hdr.sectname());
                    let out = output_section_for(&ctx.args, map, seg, sect, hdr.flags);
                    let id = out.map(|(out, flags_name)| match by_out.get(&out) {
                        Some(&id) => id,
                        None => {
                            let flags = first_member_flags(ctx, &hdr, text, out, flags_name);
                            let id = add_output_section(ctx, out.0, out.1, flags);
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
            last_hdr = hdr_ptr;
            last_osec = id;
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

/// The flags of a new output section, `out`, from those of its first
/// member, an input section with header `hdr` whose flags follow the
/// name `flags_name` (see output_section_for). The first member
/// decides, as in ld-prime: code after data in a section doesn't make
/// it code. An empty member counts if it names an atom (see
/// bare_sections), in -r too.
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
    seg: &'static str,
    sect: &str,
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
/// the alignment with a warning (an x86-64 .align 16 asks for 64KB) -
/// but not in a -static or -preload image, which no dyld maps: ld-prime
/// starts the section's segment on the alignment there (see
/// lay_out_segments).
fn finish_section_alignments<E: Target>(ctx: &mut Context<E>, text: SectionName) {
    let capped = !ctx.args.relocatable && !ctx.args.static_link;
    let max = ctx.args.segment_align.max(1).trailing_zeros();
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
                    hdr.segname,
                    hdr.sectname,
                    1u64 << hdr.p2align,
                    1u64 << p2align
                );
            }
            hdr.p2align = p2align;
        }
        if capped && hdr.p2align > max {
            crate::warn!(
                "reducing alignment of section {},{} from 0x{:x} to 0x{:x} because it exceeds segment maximum alignment",
                hdr.segname,
                hdr.sectname,
                1u64 << hdr.p2align,
                1u64 << max
            );
            hdr.p2align = max;
        }
    }

    // dyld wants its code at a stable address, whatever its load
    // commands take: ld64 aligns its __text to 4 KiB, on any target and
    // whatever the inputs or -sectalign ask, and leaves no room between
    // the load commands and it (see chunks::header_pad).
    if ctx.args.is_dylinker() {
        let id = find_output_section(ctx, text).unwrap();
        ctx.output_sections[id.index()].hdr.p2align = 12;
    }
}

/// Orders each output section's members: the atoms -order_file names
/// first, cold code last, and the rest in input order. Thread-local
/// zero fill goes by size instead.
fn sort_section_members<E: Target>(ctx: &mut Context<E>) {
    // ld-prime lays out a final image's __thread_bss by atom size,
    // smallest first and in input order among equals, whatever
    // -order_file says. An atom's size runs to the next one in its
    // object, padding included.
    let is_tbss = |osec: &OutputSection| osec.hdr.flags & SECTION_TYPE == S_THREAD_LOCAL_ZEROFILL;
    if !ctx.args.relocatable {
        for osec in ctx.output_sections.iter_mut().filter(|osec| is_tbss(osec)) {
            osec.members.sort_by_key(|&id| ctx.isecs[id as usize].size);
        }
    }

    // -order_file moves the atoms it names to the front of their
    // output sections, in the file's order; everything else keeps its
    // input order behind them. A stable sort by rank does both.
    if let Some(ranks) = order_file_ranks(ctx) {
        for osec in ctx.output_sections.iter_mut().filter(|osec| !is_tbss(osec)) {
            osec.members.sort_by_key(|&id| ranks[id as usize]);
        }
    }

    // Cold code last: clang marks the rarely-run part it splits off a
    // function (foo.cold.1, and the function it came from) N_COLD_FUNC,
    // and ld64 lays those atoms out after every other atom of their
    // section - in final images and -r outputs alike - so hot code
    // stays dense.
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
        ctx.objc_stubs.hdr.size = ctx.objc_stubs.symbols.len() as u64 * E::OBJC_STUB_SIZE;
        // 32-byte stubs on arm64; ld-prime leaves x86-64's byte-aligned.
        if E::CPUTYPE == crate::macho::CPU_TYPE_X86_64 {
            ctx.objc_stubs.hdr.p2align = 0;
        }
        ctx.chunks.push(ChunkId::ObjcStubs);
    }

    let methname_size = ctx.objc_stubs.methname_data.len() as u64;
    let selrefs_size =
        (ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len()) as u64 * 8;
    let map = SectionMap::final_link(ctx);
    if methname_size > 0 {
        let ((seg, sect), _) =
            output_section_for(&ctx.args, map, "__TEXT", "__objc_methname", S_CSTRING_LITERALS)
                .unwrap();
        let id =
            tail_section(ctx, seg, sect, S_CSTRING_LITERALS, 0, Tail::ObjcMethname, methname_size);
        ctx.objc_stubs.methname = Some(id);
    }
    if selrefs_size > 0 {
        let ((seg, sect), (flags_seg, flags_sect)) =
            output_section_for(&ctx.args, map, "__DATA", "__objc_selrefs", S_LITERAL_POINTERS)
                .unwrap();
        // A slot keeps the alignment of the inputs it took over.
        let p2align = (ctx.objc_stubs.absorbed.iter())
            .map(|&(synth, _)| ctx.isecs[synth as usize].p2align as u32)
            .fold(3, u32::max);
        let id = tail_section(
            ctx,
            seg,
            sect,
            output_section_flags(flags_seg, flags_sect, S_LITERAL_POINTERS, true, false),
            p2align,
            Tail::ObjcSelrefs,
            selrefs_size,
        );
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
/// retires some after their first placement.
fn lay_out_objc_method_lists<E: Target>(ctx: &mut Context<E>) {
    if ctx.objc_methlist.lists.is_empty() {
        return;
    }
    let mut name_of: hashbrown::HashMap<u32, &'static str> = hashbrown::HashMap::new();
    let syms = ctx.symbols.syms.iter().filter_map(|sym| Some((sym.name(), sym.input_section()?)));
    // (The lists category merging builds are named as extra locals.)
    for (name, isec) in syms.chain(ctx.extra_local_syms.iter().copied()) {
        let r = ctx.resolve_isec(isec as usize) as u32;
        let e = name_of.entry(r).or_insert(name);
        if name < *e {
            *e = name;
        }
    }
    let mut order: Vec<usize> = (0..ctx.objc_methlist.lists.len()).collect();
    order.sort_by_key(|&i| {
        (name_of.get(&ctx.objc_methlist.lists[i].isec).copied().unwrap_or(""), i)
    });
    let mut off = 0u64;
    for i in order {
        let isec = ctx.objc_methlist.lists[i].isec as usize;
        off = align_to(off, 8);
        ctx.isecs[isec].offset = off as u32;
        off += ctx.isecs[isec].size as u64;
    }
    ctx.objc_methlist.hdr.size = off;
    ctx.chunks.push(ChunkId::ObjcMethlist);
    for i in 0..ctx.objc_methlist.lists.len() {
        let isec = ctx.objc_methlist.lists[i].isec as usize;
        ctx.isecs[isec].set_output_section(ChunkId::ObjcMethlist);
    }
}

/// Adds the sections -sectcreate makes from files, and the empty ones
/// -add_empty_section asks for, which give tools a named anchor (their
/// section$start/end addresses) without any content.
fn add_sectcreate_sections<E: Target>(ctx: &mut Context<E>) {
    let sectcreate = std::mem::take(&mut ctx.args.sectcreate);
    for (seg, sect, path) in &sectcreate {
        let data = std::fs::read(path).unwrap_or_else(|e| {
            let errno = crate::error::errno_text(&e);
            fatal!("file cannot be open()ed, {errno} path={}", path.display())
        });
        let segname: &'static str = String::leak(seg.clone());
        add_sectcreate(ctx, SectCreateSection::new(segname, sect, Vec::leak(data), true));
    }
    ctx.args.sectcreate = sectcreate;

    let empties = std::mem::take(&mut ctx.args.add_empty_section);
    for (seg, sect) in &empties {
        let segname: &'static str = String::leak(seg.clone());
        add_sectcreate(ctx, SectCreateSection::new(segname, sect, &[], true));
    }
    ctx.args.add_empty_section = empties;
}

/// Merges the objects' __objc_imageinfo records into the image's: the
/// Swift version must agree, the Swift language version is the newest,
/// and the category-class-properties bit holds only if every
/// Objective-C object has it.
fn merge_objc_image_info<E: Target>(ctx: &mut Context<E>) {
    let infos: Vec<u32> =
        ctx.objs.iter().filter(|o| o.is_alive).filter_map(|o| o.objc_image_info).collect();
    if infos.is_empty() {
        return;
    }
    let mut swift_version = 0;
    for &flags in &infos {
        let v = (flags >> 8) & 0xff;
        if swift_version == 0 {
            swift_version = v;
        } else if v != 0 && v != swift_version {
            error!("incompatible __objc_imageinfo swift versions");
        }
    }
    let lang = infos.iter().map(|f| f >> 16).max().unwrap();
    let cat = infos.iter().all(|f| f & 0x40 != 0);
    let flags = (lang << 16) | (swift_version << 8) | if cat { 0x40 } else { 0 };

    ctx.objc_imageinfo.flags = flags;
    ctx.objc_imageinfo.hdr.segname = data_seg(ctx);
    ctx.objc_imageinfo.hdr.size = 8;
    ctx.chunks.push(ChunkId::ObjcImageInfo);
}

/// Lays out __eh_frame, the surviving DWARF unwind records: live CIEs
/// first, then FDEs. Their offsets are needed before layout, because
/// the __unwind_info encoding embeds each FDE's offset.
fn lay_out_eh_frame<E: Target>(ctx: &mut Context<E>) {
    // FDEs of folded copies duplicate their leader's; drop them, and
    // remap the unwind records' FDE indices around the removals as
    // the dead-strip pass does (a record left pointing past the
    // shortened table crashed the encoder).
    let mut fde_map = vec![usize::MAX; ctx.fdes.len()];
    let mut kept_fdes = Vec::new();
    let fdes = std::mem::take(&mut ctx.fdes);
    for (i, fde) in fdes.into_iter().enumerate() {
        if ctx.isecs[fde.isec].replacement == crate::input_sections::NO_REPLACEMENT {
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

    if ctx.fdes.is_empty() {
        return;
    }
    for fde in &ctx.fdes {
        ctx.cies[fde.cie as usize].is_alive = true;
    }
    let mut off = 0;
    for cie in &mut ctx.cies {
        if cie.is_alive {
            cie.output_offset = off;
            off += cie.data.len() as u32;
        }
    }
    for fde in &mut ctx.fdes {
        fde.output_offset = off;
        off += fde.data.len() as u32;
    }

    ctx.eh_frame.hdr.flags = output_section_flags("__TEXT", "__eh_frame", 0, true, false);
    ctx.eh_frame.hdr.size = off as u64;
    ctx.chunks.push(ChunkId::EhFrame);
}

/// Sorts the chunks into file order: the standard segment order, and
/// section ranks within a segment. Sections of one rank follow the
/// order their first input section was seen in (see
/// section_first_seen), as ld-prime lays them out. Segment ranks honor
/// -segment_order, then the standard order; segments stay together,
/// and __LINKEDIT is always last.
fn sort_chunks<E: Target>(ctx: &mut Context<E>) {
    let section_first_seen = section_first_seen(ctx);
    let mut order = ctx.chunks.clone();
    let mut first_seen: hashbrown::HashMap<&'static str, usize> = hashbrown::HashMap::new();
    for &id in &order {
        let n = first_seen.len();
        first_seen.entry(ctx.chunk_header(id).segname).or_insert(n);
    }
    let segment_order = &ctx.args.segment_order;
    order.sort_by_key(|&id| {
        let hdr = ctx.chunk_header(id);
        // Code in __TEXT_EXEC follows __TEXT. The __DATA_CONST of an
        // image no dyld loads (a -static one with -data_const or in the
        // shared region, a kext) comes after __DATA, as ld-prime
        // places it.
        let standard = match hdr.segname {
            "__TEXT" | "__TEXT_EXEC" => 0,
            "__DATA_CONST" if !ctx.args.without_dyld() => 1,
            "__DATA" => 2,
            "__DATA_CONST" => 3,
            _ => 4,
        };
        // -segment_order orders the rest: __TEXT, which holds the
        // mach header, stays first and __LINKEDIT last. A -preload
        // image's header precedes its segments but lies in none, and
        // its __TEXT goes where the list says.
        let seg_rank = match (id, hdr.segname) {
            (ChunkId::MachHeader, _) if ctx.args.preload => 0,
            (_, "__TEXT") if !ctx.args.preload => 0,
            (_, "__LINKEDIT") => usize::MAX,
            (_, name) => match segment_order.iter().position(|s| s == name) {
                Some(i) => 1 + i,
                None => 1 + segment_order.len() + standard,
            },
        };
        let seg_rank = (seg_rank, first_seen[hdr.segname]);
        let sect_rank = match id {
            ChunkId::MachHeader => 0,
            ChunkId::UnwindInfo => 100,
            ChunkId::EhFrame => 101,
            ChunkId::CodeSignature => u32::MAX,
            _ => shared_region_text_rank(ctx, hdr)
                .unwrap_or_else(|| 1 + output_section_rank(hdr.segname, &hdr.sectname, hdr.flags)),
        };
        let seen = match id {
            ChunkId::Output(osec) => section_first_seen[osec.index()],
            _ => u64::MAX,
        };
        // Zero-fill sections go last in their segment so that they don't
        // occupy file space in the middle of it.
        (seg_rank, hdr.is_zerofill(), listed_section_rank(ctx, hdr), sect_rank, seen)
    });
    ctx.chunks = order;
}

/// When ld-prime first sees each output section, by output section: at
/// the object and section ordinal of its first input section - or, for
/// the synthesized __common, at the first object with a common symbol.
/// mold's own subsections don't count, nor the input selector names
/// the objc_msgSend$ stubs absorb (see is_stub_selector_name).
fn section_first_seen<E: Target>(ctx: &Context<E>) -> Vec<u64> {
    let mut first_seen: Vec<u64> = vec![u64::MAX; ctx.output_sections.len()];
    let stub_sels: hashbrown::HashSet<&[u8]> =
        ctx.objc_stubs.symbols.iter().map(|(_, sel)| sel.as_bytes()).collect();
    for (i, isec) in ctx.isecs.iter().enumerate() {
        if ctx.is_internal(isec.file as usize) || is_stub_selector_name(ctx, isec, &stub_sels) {
            continue;
        }
        // A copy merged into another input's (a literal, the losing
        // copy of a weak definition) places nothing: ld-prime places a
        // section by the atoms it keeps. One a synthesized record took
        // over counts where it was.
        let kept = ctx.resolve_isec(i);
        if kept != i && !ctx.is_internal(ctx.isecs[kept].file as usize) {
            continue;
        }
        let Some(ChunkId::Output(id)) = ctx.isecs[kept].output_section() else {
            continue;
        };
        let key = ((isec.file as u64) << 32) | isec.shndx as u64;
        let slot = &mut first_seen[id.index()];
        *slot = (*slot).min(key);
    }
    if let Some(obj) = ctx.common_first_obj {
        for (i, osec) in ctx.output_sections.iter().enumerate() {
            if osec.hdr.segname == "__DATA" && osec.hdr.sectname == "__common" {
                first_seen[i] = ((obj as u64) << 32) | u32::MAX as u64;
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
        segments.push(OutputSegment::new("__PAGEZERO"));
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
    hdr.segname == "__TEXT" && hdr.sectname == "__text"
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
        let others =
            sects.iter().filter(|hdr| !list.contains(&hdr.sectname) && !is_text_section(hdr));
        let order: Vec<&&ChunkHeader> = listed.chain(others).collect();
        if let Some(i) = order.iter().position(|hdr| hdr.is_zerofill())
            && order[i..].iter().any(|hdr| !hdr.is_zerofill())
        {
            fatal!(
                "{} is zero-fill, it should be ordered at the end of the segment {seg}, or alongside other zero-fill sections",
                order[i].sectname
            );
        }
    }
}

/// An image bound for the shared region (see resolve_shared_region)
/// may not carry interposing tuples, which the dyld shared cache
/// builder refuses. ld-prime finds them as dyld does - a section named
/// __interpose in a segment whose name starts with __DATA or __AUTH,
/// by its final name - and rejects even an empty one.
fn check_interposing<E: Target>(ctx: &Context<E>) {
    if !ctx.args.shared_region {
        return;
    }
    let is_interpose = |hdr: &&ChunkHeader| {
        hdr.is_sect
            && hdr.sectname == "__interpose"
            && (hdr.segname.starts_with("__DATA") || hdr.segname.starts_with("__AUTH"))
    };
    if let Some(hdr) = ctx.chunks.iter().map(|&id| ctx.chunk_header(id)).find(is_interpose) {
        error!(
            "Shared cache eligible dylib cannot use interposing tuples (found in '{} {}').  \
             Remove interposing tuples, or opt out of the shared cache using the build setting \
             'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag '-not_for_dyld_shared_cache')",
            hdr.segname, hdr.sectname
        );
    }
}

/// The segment of the mach header: __TEXT, which -rename_segment
/// moves only in a -static image. A dynamic image's header stays in
/// __TEXT, where dyld looks for it.
pub(crate) fn header_segment<E: Target>(ctx: &Context<E>) -> &'static str {
    if ctx.args.static_link { renamed_segment(&ctx.args, "__TEXT") } else { "__TEXT" }
}

/// The name of the __text section a final image always has: it moves
/// with -text_exec like the code, and -rename_section and
/// -rename_segment rename it like any section - but -rename_segment
/// __TEXT leaves it with the mach header.
fn text_section_name<E: Target>(ctx: &Context<E>) -> SectionName {
    let (seg, sect) =
        SectionMap::final_link(ctx).builtin_name(("__TEXT", "__text"), S_ATTR_PURE_INSTRUCTIONS);
    let is_renamed = ctx.args.rename_sections.iter().any(|(s, t, _, _)| s == seg && t == sect);
    if seg == "__TEXT" && !is_renamed {
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
/// its segment. (The output sections of input sections got their
/// renamed names when created.) A -sectcreate __DATA,__interpose moves
/// to __DATA_CONST like an input section (see SectionMap::renamed).
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
        if !hdr.is_sect || matches!(id, ChunkId::Output(_) | ChunkId::UnwindInfo) {
            continue;
        }
        let (seg, sect) = map.renamed(&ctx.args, (hdr.segname, static_name(&hdr.sectname)));
        let hdr = ctx.chunk_header_mut(id);
        hdr.segname = seg;
        hdr.sectname = sect.to_string();
    }
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
        let (_, _, seg, sect) = &ctx.boundary_syms[i];
        let Some(sect) = sect else {
            let seg = renamed_segment(&ctx.args, static_name(seg));
            ctx.boundary_syms[i].2 = seg.to_string();
            continue;
        };
        let flags = boundary_section_flags(seg, sect);
        let name = map.boundary_name((static_name(seg), static_name(sect)));
        let (seg, sect) = map.renamed(&ctx.args, name);
        ctx.boundary_syms[i].2 = seg.to_string();
        ctx.boundary_syms[i].3 = Some(sect.to_string());
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
    let mut syms: Vec<(&str, &str)> = ctx
        .boundary_syms
        .iter()
        .filter(|(_, _, seg, sect)| sect.is_none() && !ctx.segments.iter().any(|s| s.name == seg))
        .map(|(id, _, seg, _)| (ctx.symbols[*id].name(), seg.as_str()))
        .collect();
    syms.sort_unstable();
    let mut missing: Vec<&'static str> = Vec::new();
    for (_, seg) in syms {
        if !missing.contains(&seg) {
            missing.push(static_name(seg));
        }
    }
    let linkedit = ctx.segments.len() - 1;
    ctx.segments.splice(linkedit..linkedit, missing.into_iter().map(OutputSegment::new));
}

/// A static executable's -stack_size stack: a segment of address space
/// alone before __LINKEDIT, pinned where resolve_stack_size says.
fn add_stack_segment<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.static_link && ctx.args.stack_size != 0 {
        let linkedit = ctx.segments.len() - 1;
        ctx.segments.insert(linkedit, OutputSegment::new("__UNIXSTACK"));
    }
}

/// The flags ld-prime gives a section only a section$start$ or
/// section$end$ symbol makes, by the name the symbol gives (before any
/// move or rename): a standard section's, though the initializer and
/// terminator lists are plain data then, and none for another name.
fn boundary_section_flags(segname: &str, sectname: &str) -> u32 {
    match (segname, sectname) {
        ("__DATA", "__mod_init_func" | "__mod_term_func") => S_REGULAR,
        _ => standard_section_flags(segname, sectname).unwrap_or(S_REGULAR),
    }
}

/// Sizes the stubs, the lazy-binding helper and pointers, the
/// lazy-load helpers and slots, and the GOT (with __weak_got split off,
/// see GotSection), and adds the ones in use to the output.
fn add_stub_and_got_chunks<E: Target>(ctx: &mut Context<E>) {
    if !ctx.stubs.symbols.is_empty() {
        if ctx.args.text_exec {
            ctx.stubs.hdr.segname = "__TEXT_EXEC";
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
        ctx.stub_helper.hdr.size = E::STUB_HELPER_HEADER_SIZE
            + ctx.stubs.lazy.len() as u64 * E::STUB_HELPER_ENTRY_SIZE
            - E::STUB_HELPER_ENTRY_PADDING;
        ctx.chunks.push(ChunkId::StubHelper);
        // In the shared region, dyld binds them all at load, and the
        // section joins the read-only data.
        if ctx.args.shared_region {
            ctx.lazy_ptrs.hdr.segname = data_seg(ctx);
        }
        ctx.lazy_ptrs.hdr.size = ctx.stubs.lazy.len() as u64 * 8;
        ctx.chunks.push(ChunkId::LazyPtrs);
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
    // symbol table all the same).
    let flags = if ctx.args.is_kext() { S_REGULAR } else { S_NON_LAZY_SYMBOL_POINTERS };
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
    // its relocations, by which kmutil links it.
    if !ctx.args.without_dyld() {
        ctx.chunks.push(ChunkId::ChainedFixups);
        ctx.chunks.push(ChunkId::RebaseInfo);
        ctx.chunks.push(ChunkId::BindInfo);
        ctx.chunks.push(ChunkId::WeakBindInfo);
        ctx.chunks.push(ChunkId::LazyBindInfo);
        ctx.chunks.push(ChunkId::ExportTrie);
    } else if ctx.use_chained_fixups() {
        ctx.chunks.push(ChunkId::ChainedFixups);
    } else if ctx.args.no_fixup_chains {
        ctx.chunks.push(ChunkId::RebaseInfo);
        ctx.chunks.push(ChunkId::WeakBindInfo);
    } else if ctx.args.pie || ctx.args.is_kext() {
        ctx.chunks.push(ChunkId::LocalRelocs);
    }
    if ctx.args.shared_region {
        ctx.chunks.push(ChunkId::SplitInfo);
    }
    if !ctx.lazy_load_info.dylibs.is_empty() {
        ctx.chunks.push(ChunkId::LazyLoadInfo);
    }
    ctx.chunks.push(ChunkId::FunctionStarts);
    if ctx.args.data_in_code_info {
        ctx.chunks.push(ChunkId::DataInCode);
    }
    ctx.chunks.push(ChunkId::Symtab);
    if ctx.args.is_kext() {
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
    let mut msg = format!(
        "section {},{} has a conflicting zerofill flag defined in {} but missing in:",
        osec.hdr.segname,
        osec.hdr.sectname,
        name(defined_in)
    );
    for file in missing_in {
        msg += &format!("\n  {}", name(file));
    }
    crate::warn!("{msg}");
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
    decls
        .into_iter()
        .filter_map(|(sym, (_, obj))| Some((ctx.symbols[sym].input_section()?, obj)))
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
    let has_text = !ctx.args.preload && ctx.segments.iter().any(|s| s.name == "__TEXT");
    if has_text && order.iter().position(|s| s == "__TEXT").is_some_and(|i| i != text_pos) {
        crate::warn!(
            "-segment_order of __TEXT is ignored, the segment must be ordered {text_place}"
        );
    }
    if order.iter().position(|s| s == "__LINKEDIT").is_some_and(|i| i != order.len() - 1) {
        crate::warn!("-segment_order of __LINKEDIT is ignored, the segment must be ordered last");
    }
    for seg in &ctx.segments {
        let fixed = match seg.name {
            "__PAGEZERO" | "__LINKEDIT" => true,
            "__TEXT" => !ctx.args.preload,
            _ => false,
        };
        if !fixed && !order.iter().any(|s| s == seg.name) {
            crate::warn!("-segment_order should list all segments, {} is missing", seg.name);
        }
    }
}

/// Adds a synthesized section with fixed contents to the output.
fn add_sectcreate<E: Target>(ctx: &mut Context<E>, sec: SectCreateSection) {
    let idx = ctx.sectcreate_sections.len() as u32;
    ctx.sectcreate_sections.push(sec);
    ctx.chunks.push(ChunkId::SectCreate(idx));
}

/// Reads the -order_file lists and ranks every subsection: the
/// subsection defining the file's first symbol gets rank 0 and so on;
/// unlisted subsections rank last. ld64's format is one
/// [arch:][object:]symbol per line with #-comments; the qualifiers
/// narrow a match, which this implementation approximates by
/// matching the bare symbol name.
fn order_file_ranks<E: Target>(ctx: &Context<E>) -> Option<Vec<u64>> {
    if ctx.args.order_files.is_empty() {
        return None;
    }

    // A line is [arch:][object-file:]symbol. An arch qualifier gates
    // the whole line; an object qualifier narrows the match to
    // symbols from that file (compared by leaf name, as ld64 does).
    const ARCHS: [&str; 6] = ["arm64", "arm64e", "x86_64", "i386", "armv7", "ppc"];
    let mut rank_of: std::collections::HashMap<String, Vec<(Option<String>, u64)>> =
        std::collections::HashMap::new();
    let mut next = 0u64;
    for path in &ctx.args.order_files {
        // ld64 links on without the order a missing file would give.
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                let errno = crate::error::errno_text(&e);
                crate::warn!("order file '{}' could not be opened, {errno}", path.display());
                continue;
            }
        };
        for line in text.lines() {
            let mut line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some((first, rest)) = line.split_once(':')
                && ARCHS.contains(&first.trim())
            {
                if first.trim() != E::NAME {
                    continue;
                }
                line = rest.trim();
            }
            let (file, name) = match line.split_once(':') {
                Some((file, name)) => (Some(file.trim().to_string()), name.trim()),
                None => (None, line),
            };
            rank_of.entry(name.to_string()).or_default().push((file, next));
            next += 1;
        }
    }

    let mut ranks = vec![u64::MAX; ctx.isecs.len()];
    for sym in &ctx.symbols.syms {
        let Some(FileId::Obj(obj)) = sym.file() else {
            continue;
        };
        let obj = obj as usize;
        let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
        let Some(entries) = rank_of.get(sym.name()) else {
            continue;
        };
        let leaf = ctx.objs[obj].mf.name.file_name().map_or(&[][..], |f| f.as_bytes());
        for (file, r) in entries {
            let applies = match file {
                Some(f) => {
                    leaf == f.as_bytes()
                        || path_bytes(&ctx.objs[obj].mf.name).ends_with(f.as_bytes())
                }
                None => true,
            };
            if applies {
                let isec = ctx.resolve_isec(isec);
                ranks[isec] = ranks[isec].min(*r);
            }
        }
    }
    Some(ranks)
}
