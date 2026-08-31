//! This file contains code to read DWARF debug info to create .gdb_index.
//!
//! .gdb_index is an optional section to speed up GNU debugger. It contains
//! two maps: 1) a map from function/variable/type names to compunits, and
//! 2) a map from function address ranges to compunits. gdb uses these
//! maps to quickly find a compunit given a name or an instruction pointer.
//!
//! (Terminology: a DWARF "unit" is a self-contained sequence of debug
//! entries. A compilation unit (CU) describes a source file and owns
//! address ranges. A type unit (TU) describes one shareable type and is
//! identified by a signature. DWARF 4 puts TUs in .debug_types; DWARF 5
//! puts DW_UT_type units in .debug_info. This file reads CUs from DWARF 2
//! to 5 and TUs from DWARF 5.)
//!
//! .gdb_index is not mandatory. All the information in .gdb_index is
//! also in other debug info sections. You can actually create an
//! executable without .gdb_index and later add it using the
//! `gdb-add-index` post-processing tool that comes with gdb.
//!
//! Post-relocated debug section contents are needed to create a
//! .gdb_index. Therefore, we create it after relocating all the other
//! sections. The size of the section is also hard to estimate before
//! applying relocations to debug info sections, so a .gdb_index is
//! placed at the very end of the output file, even after the section
//! header.
//!
//! The mapping from names to compunits is 1:n while the mapping from
//! address ranges to compunits is 1:1. That is, two object files may
//! define the same type name, while there should be no two functions
//! that overlap with each other in memory.
//!
//! .gdb_index contains an on-disk hash table for names, so gdb can
//! lookup names without loading all strings into memory and construct an
//! in-memory hash table.
//!
//! Names are in .debug_gnu_pubnames and .debug_gnu_pubtypes input
//! sections. These sections are created if `-ggnu-pubnames` is given.
//! Besides names, these sections contain attributes for each name so
//! that gdb can distinguish type names from function names, for example.
//!
//! A compunit contains one or more function address ranges. If an
//! object file is compiled without -ffunction-sections, it contains
//! only one .text section and therefore contains a single address range.
//! Such range is typically stored directly to the compunit.
//!
//! If an object file is compiled with -ffunction-sections, it contains
//! more than one .text section, and it has as many address ranges as
//! the number of .text sections. Such discontiguous address ranges are
//! stored to .debug_ranges in DWARF 2/3/4 and .debug_rnglists/.debug_addr
//! in DWARF 5.
//!
//! .debug_info section contains DWARF debug info. Although we don't need
//! to parse the whole .debug_info section to read address ranges, we
//! have to do a little bit. DWARF is complicated and often handled using
//! a library such as libdwarf. But we don't use any library because we
//! don't want to add an extra run-time dependency just for --gdb-index.
//!
//! This page explains the format of .gdb_index:
//! https://sourceware.org/gdb/onlinedocs/gdb/Index-Section-Format.html
//!
//! `.gdb_index` generation for `--gdb-index`.
//!
//! `.gdb_index` speeds up gdb start-up. It maps the names of functions,
//! variables and types to the units defining them, and address ranges to
//! compilation units, so that gdb can find the unit for a name or a PC
//! without reading all of the debug info first. The format is described
//! at https://sourceware.org/gdb/onlinedocs/gdb/Index-Section-Format.html.
//!
//! Names come from `.debug_gnu_pubnames` and `.debug_gnu_pubtypes`, which
//! compilers emit under `-ggnu-pubnames`; those sections are consumed
//! rather than copied to the output. Address ranges come from the
//! compilation units in `.debug_info`, possibly through `.debug_ranges`,
//! `.debug_rnglists` and `.debug_addr`, and are only known once
//! relocations have been applied. The index is therefore assembled in
//! three stages: units and names are read from the inputs, the name
//! table is built once the order of units in the output is fixed, and
//! the address table is written after everything else, at the very end
//! of the file.

// DWARF constants keep the spelling of the specification.
#![allow(non_upper_case_globals)]

use rayon::prelude::*;

use crate::arch::Arch;
use crate::context::Context;
use crate::elf::*;
use crate::error::Diagnostics;
use crate::fatal;
use crate::output_chunks::ChunkId;
use crate::output_file::{split_at_offsets, OutputFile};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::util::concurrent_map::{ConcurrentMap, FrozenMap, MapEntryRef};
use crate::util::hyperloglog::HyperLogLog;
use crate::util::perf::Timer;
use crate::util::read_uleb;

/// A public name and its GNU kind before the name is interned in GdbNameMap.
#[derive(Clone, Copy)]
#[repr(C)]
struct NameType {
    // The low byte stores the type. Hash collisions, including collisions in
    // the discarded high byte, are resolved by comparing the complete name.
    hash_and_type: u64,
    name: usize,
}

impl NameType {
    fn new(hash: u64, kind: u8, name: &'static [u8]) -> NameType {
        NameType {
            hash_and_type: (hash << 8) | kind as u64,
            name: name.as_ptr() as usize,
        }
    }

    fn hash(self) -> u64 {
        self.hash_and_type >> 8
    }

    fn kind(self) -> u8 {
        self.hash_and_type as u8
    }

    fn same_name(self, other: NameType) -> bool {
        self.hash_and_type == other.hash_and_type
            // SAFETY: public names point into NUL-terminated debug sections
            // that remain live for the complete link.
            && unsafe {
                libc::strcmp(
                    (self.name as *const u8).cast(),
                    (other.name as *const u8).cast(),
                ) == 0
            }
    }
}

/// The interned name's map entry and reserved slot in its type vector.
#[derive(Clone, Copy)]
#[repr(C)]
struct IndexedName {
    entry: NameEntryRef,
    type_vector_idx: u32,
    kind: u8,
}

/// A name record is read as a NameType and replaced with an IndexedName after
/// the name has been interned. The union avoids retaining both forms for the
/// millions of public names in a large debug link.
#[derive(Clone, Copy)]
union NameRecord {
    nametype: NameType,
    indexed: IndexedName,
}

impl NameRecord {
    fn new(hash: u64, kind: u8, name: &'static [u8]) -> NameRecord {
        NameRecord {
            nametype: NameType::new(hash, kind, name),
        }
    }

    fn nametype(self) -> NameType {
        // SAFETY: called only before the interning phase replaces this record.
        unsafe { self.nametype }
    }

    fn indexed(self) -> IndexedName {
        // SAFETY: called only after every record has been interned.
        unsafe { self.indexed }
    }

    fn set_indexed(&mut self, indexed: IndexedName) {
        self.indexed = indexed;
    }
}

const _: () = assert!(std::mem::size_of::<NameRecord>() == 16);

/// CU metadata carried from input parsing through final index serialization.
/// CUs own address ranges; both CUs and TUs below may own public names.
struct Compunit {
    /// Initially relative to the input contribution selected by file_idx/shndx;
    /// rebased to the output .debug_info section in build_gdb_index_tables.
    offset: u64,
    size: u64,
    file: u32,
    shndx: u32,
    names: Vec<NameRecord>,
    ranges: Vec<(u64, u64)>,
}

/// TU metadata used for the .gdb_index type-unit list. TUs have no address
/// ranges because executable code is attributed to compilation units.
struct Typeunit {
    /// `offset` is rebased like Compunit::offset. `type_die_offset` remains
    /// relative to the unit, as required by the .gdb_index type-unit table.
    offset: u64,
    type_die_offset: u64,
    signature: u64,
    file: u32,
    shndx: u32,
    names: Vec<NameRecord>,
}

/// Build-time state for one unique name in the .gdb_index symbol table.
struct NameEntry {
    /// gdb's own hash of the name, which decides its hash table slot.
    gdb_hash: u32,
    /// The number of units the name occurs in.
    count: AtomicU32,
    /// Offsets in the constant pool.
    type_vector_offset: u32,
    name_offset: u32,
}

/// A stable pointer to a value in the GDB name map.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct NameEntryRef(NonNull<NameEntry>);

impl NameEntryRef {
    fn new(entry: &NameEntry) -> NameEntryRef {
        NameEntryRef(NonNull::from(entry))
    }

    fn get(self, _owner: &FrozenMap<NameEntry>) -> &NameEntry {
        // SAFETY: values live at stable addresses in the map's mmap allocation.
        // The owner keeps that allocation live for the returned reference.
        unsafe { self.0.as_ref() }
    }
}

// SAFETY: NameEntryRef is only dereferenced while the owning map is live, and
// NameEntry is shared between threads through its atomic fields.
unsafe impl Send for NameEntryRef {}
unsafe impl Sync for NameEntryRef {}

#[derive(Clone, Copy, Default)]
/// Byte counts accumulated by the parallel constant-pool layout scan.
struct PoolSize {
    type_bytes: u32,
    name_bytes: u32,
}

/// State shared by the input reader, table builder and final serialization pass.
pub struct GdbIndexData {
    cus: Vec<Compunit>,
    tus: Vec<Typeunit>,
    names: Option<FrozenMap<NameEntry>>,
    /// The names in output order.
    entries: Vec<MapEntryRef<NameEntry>>,
    type_pool_size: u32,
    name_pool_size: u32,
    ht_size: u32,
    /// The hash table followed by the constant pool, once built.
    tables: Vec<u32>,
}

/// Views word-aligned table storage as its serialized bytes.
fn words_as_bytes(words: &[u32]) -> &[u8] {
    // SAFETY: u8 has alignment one and the byte length covers the same
    // allocation without outliving it.
    unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), std::mem::size_of_val(words)) }
}

/// Normalized view of a DWARF unit header. DWARF32 and DWARF64 describe the
/// width of section offsets, not the ELF class or target address size.
struct UnitHeader {
    size: u64,
    header_size: u64,
    abbrev_offset: u64,
    type_die_offset: u64,
    signature: u64,
    version: u16,
    unit_type: u8,
    address_size: u8,
    offset_size: u8,
}

/// A cursor over DWARF data.
struct Reader<'d, 'a, E: Arch> {
    diag: &'d Diagnostics,
    data: &'a [u8],
    pos: usize,
    marker: std::marker::PhantomData<E>,
}

impl<'d, 'a, E: Arch> Reader<'d, 'a, E> {
    fn new(diag: &'d Diagnostics, data: &'a [u8], pos: usize) -> Reader<'d, 'a, E> {
        Reader {
            diag,
            data,
            pos,
            marker: std::marker::PhantomData,
        }
    }

    fn take(&mut self, n: usize) -> &'a [u8] {
        let Some(bytes) = self.data.get(self.pos..self.pos + n) else {
            fatal!(self.diag, "--gdb-index: truncated debug info");
        };
        self.pos += n;
        bytes
    }

    fn uint(&mut self, n: usize) -> u64 {
        let bytes = self.take(n);
        match n {
            1 => bytes[0] as u64,
            2 => E::Endian::read_u16(bytes) as u64,
            3 => {
                if E::IS_LITTLE_ENDIAN {
                    bytes[0] as u64 | (bytes[1] as u64) << 8 | (bytes[2] as u64) << 16
                } else {
                    (bytes[0] as u64) << 16 | (bytes[1] as u64) << 8 | bytes[2] as u64
                }
            }
            4 => E::Endian::read_u32(bytes) as u64,
            8 => E::Endian::read_u64(bytes),
            _ => unreachable!("unsupported integer size"),
        }
    }

    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }

    fn u16(&mut self) -> u16 {
        self.uint(2) as u16
    }

    fn u32(&mut self) -> u32 {
        self.uint(4) as u32
    }

    fn u64(&mut self) -> u64 {
        self.uint(8)
    }

    /// A section offset, whose width depends on the DWARF32/64 format.
    fn offset(&mut self, offset_size: u8) -> u64 {
        self.uint(offset_size as usize)
    }

    fn uleb(&mut self) -> u64 {
        let mut rest = &self.data[self.pos..];
        let before = rest.len();
        let val = read_uleb(&mut rest);
        self.pos += before - rest.len();
        val
    }

    fn cstr(&mut self) -> &'a [u8] {
        let rest = &self.data[self.pos..];
        // SAFETY: strnlen reads at most rest.len() bytes from the slice.
        let len = unsafe { libc::strnlen(rest.as_ptr().cast(), rest.len()) };
        if len == rest.len() {
            fatal!(self.diag, "--gdb-index: unterminated string in debug info");
        }
        self.pos += len + 1;
        &rest[..len]
    }
}

fn parse_unit_header<E: Arch>(diag: &Diagnostics, data: &[u8], pos: usize) -> UnitHeader {
    // The first word is either a DWARF32 unit length or DWARF64's reserved
    // marker. unit_length excludes its own encoding: four bytes in DWARF32, or
    // the four-byte marker plus eight-byte length in DWARF64.
    let mut r = Reader::<E>::new(diag, data, pos);
    let mut unit_length = r.u32() as u64;
    let mut initial_length_size = 4;
    let mut offset_size = 4;
    if unit_length == u32::MAX as u64 {
        unit_length = r.u64();
        initial_length_size = 12;
        offset_size = 8;
    }

    let version = r.u16();
    if version > 5 {
        fatal!(
            diag,
            "--gdb-index: DWARF version {version} is not supported"
        );
    }

    let mut hdr = UnitHeader {
        size: unit_length + initial_length_size,
        header_size: 0,
        abbrev_offset: 0,
        type_die_offset: 0,
        signature: 0,
        version,
        unit_type: DW_UT_compile as u8,
        address_size: 0,
        offset_size,
    };

    if version < 5 {
        hdr.abbrev_offset = r.offset(offset_size);
        hdr.address_size = r.u8();
    } else {
        hdr.unit_type = r.u8();
        hdr.address_size = r.u8();
        hdr.abbrev_offset = r.offset(offset_size);
        match hdr.unit_type as u32 {
            DW_UT_skeleton | DW_UT_split_compile => {
                r.u64(); // dwo_id
            }
            DW_UT_type | DW_UT_split_type => {
                hdr.signature = r.u64();
                hdr.type_die_offset = r.offset(offset_size);
            }
            _ => {}
        }
    }
    hdr.header_size = (r.pos - pos) as u64;
    hdr
}

/// The address ranges a compilation unit covers.
struct RangeSections<'a> {
    info: &'a [u8],
    abbrev: &'a [u8],
    ranges: &'a [u8],
    addr: &'a [u8],
    rnglists: &'a [u8],
}

/// The first DIE refers to an abbreviation by its ULEB128 code. Walk the
/// unit's abbreviation table to find the attribute forms for that DIE.
fn find_cu_abbrev<'d, 'a, E: Arch>(
    diag: &'d Diagnostics,
    die: &mut Reader<'d, 'a, E>,
    abbrev_section: &'a [u8],
    hdr: &UnitHeader,
) -> Reader<'d, 'a, E> {
    if hdr.address_size as usize != E::WORD_SIZE {
        fatal!(
            diag,
            "--gdb-index: unsupported address size {}",
            hdr.address_size
        );
    }
    let abbrev_code = die.uleb();
    let mut abbrev = Reader::<E>::new(diag, abbrev_section, hdr.abbrev_offset as usize);
    loop {
        let code = abbrev.uleb();
        if code == 0 {
            fatal!(diag, "--gdb-index: .debug_abbrev does not contain a record for the first .debug_info record");
        }
        let tag = abbrev.uleb(); // tag
        abbrev.u8(); // skip has_children byte
        if code == abbrev_code {
            // Found a record
            if tag != DW_TAG_compile_unit as u64 && tag != DW_TAG_skeleton_unit as u64 {
                fatal!(diag, "--gdb-index: the first entry's tag is not DW_TAG_compile_unit/DW_TAG_skeleton_unit but {tag:#x}");
            }
            return abbrev;
        }
        // Skip an uninteresting record
        loop {
            let name = abbrev.uleb();
            let form = abbrev.uleb();
            if name == 0 && form == 0 {
                break;
            }
            if form == DW_FORM_implicit_const as u64 {
                abbrev.uleb();
            }
        }
    }
}

/// .debug_info contains variable-length fields. `offset_size` is four or eight
/// bytes according to the DWARF32/DWARF64 format; Word<E> is instead the
/// target's address width. This function advances over one scalar value.
fn read_scalar<E: Arch>(diag: &Diagnostics, r: &mut Reader<E>, form: u64, offset_size: u8) -> u64 {
    match form as u32 {
        DW_FORM_flag_present => 0,
        DW_FORM_data1 | DW_FORM_flag | DW_FORM_strx1 | DW_FORM_addrx1 | DW_FORM_ref1 => {
            r.u8() as u64
        }
        DW_FORM_data2 | DW_FORM_strx2 | DW_FORM_addrx2 | DW_FORM_ref2 => r.u16() as u64,
        DW_FORM_strx3 | DW_FORM_addrx3 => r.uint(3),
        DW_FORM_data4 | DW_FORM_strx4 | DW_FORM_addrx4 | DW_FORM_ref4 => r.u32() as u64,
        DW_FORM_data8 | DW_FORM_ref8 => r.u64(),
        DW_FORM_strp | DW_FORM_sec_offset | DW_FORM_line_strp => r.offset(offset_size),
        DW_FORM_addr | DW_FORM_ref_addr => r.uint(E::WORD_SIZE),
        DW_FORM_strx | DW_FORM_addrx | DW_FORM_udata | DW_FORM_ref_udata | DW_FORM_loclistx
        | DW_FORM_rnglistx => r.uleb(),
        DW_FORM_string => {
            r.cstr();
            0
        }
        _ => fatal!(diag, "--gdb-index: unhandled debug info form: {form:#x}"),
    }
}

/// Read a range list from .debug_ranges starting at the given offset.
fn read_debug_ranges<E: Arch>(r: &mut Reader<E>, mut base: u64) -> Vec<(u64, u64)> {
    let mut vec = Vec::new();
    loop {
        let start = r.uint(E::WORD_SIZE);
        let end = r.uint(E::WORD_SIZE);
        if start == 0 && end == 0 {
            return vec;
        }
        if start == u64::MAX >> (64 - 8 * E::WORD_SIZE as u32) {
            base = end;
        } else {
            vec.push((start.wrapping_add(base), end.wrapping_add(base)));
        }
    }
}

/// Read a range list from .debug_rnglists starting at the given offset.
fn read_rnglist<E: Arch>(
    diag: &Diagnostics,
    r: &mut Reader<E>,
    addrx: &[u8],
    mut base: u64,
) -> Vec<(u64, u64)> {
    let addr_at = |i: u64| -> u64 {
        let mut a = Reader::<E>::new(diag, addrx, i as usize * E::WORD_SIZE);
        a.uint(E::WORD_SIZE)
    };
    let mut vec = Vec::new();
    loop {
        match r.u8() as u32 {
            DW_RLE_end_of_list => return vec,
            DW_RLE_base_addressx => base = addr_at(r.uleb()),
            DW_RLE_startx_endx => {
                let (a, b) = (r.uleb(), r.uleb());
                vec.push((addr_at(a), addr_at(b)));
            }
            DW_RLE_startx_length => {
                let (a, len) = (r.uleb(), r.uleb());
                let start = addr_at(a);
                vec.push((start, start + len));
            }
            DW_RLE_offset_pair => {
                let (a, b) = (r.uleb(), r.uleb());
                // If the base is 0, this address range is for an eliminated
                // section. We only emit it if it's alive.
                if base != 0 {
                    vec.push((base + a, base + b));
                }
            }
            DW_RLE_base_address => base = r.uint(E::WORD_SIZE),
            DW_RLE_start_end => {
                let (a, b) = (r.uint(E::WORD_SIZE), r.uint(E::WORD_SIZE));
                vec.push((a, b));
            }
            DW_RLE_start_length => {
                let a = r.uint(E::WORD_SIZE);
                let len = r.uleb();
                vec.push((a, a + len));
            }
            kind => fatal!(
                diag,
                "--gdb-index: unknown .debug_rnglists entry kind: {kind:#x}"
            ),
        }
    }
}

/// Returns a list of address ranges explained by a compunit at the
/// `offset` in an output .debug_info section.
///
/// .debug_info contains DWARF debug info records, so this function
/// parses DWARF. If a designated compunit contains multiple ranges, the
/// ranges are read from .debug_ranges (or .debug_rnglists for DWARF5).
/// Otherwise, a range is read directly from .debug_info (or possibly
/// from .debug_addr for DWARF5).
fn read_address_ranges<E: Arch>(
    diag: &Diagnostics,
    secs: &RangeSections,
    cu: &Compunit,
) -> Vec<(u64, u64)> {
    // Read .debug_info to find the record at a given offset.
    let hdr = parse_unit_header::<E>(diag, secs.info, cu.offset as usize);
    let mut die = Reader::<E>::new(diag, secs.info, (cu.offset + hdr.header_size) as usize);
    let mut abbrev = find_cu_abbrev::<E>(diag, &mut die, secs.abbrev, &hdr);

    // Now, read debug info records.
    let mut low_pc: Option<(u64, u64)> = None;
    let mut high_pc: Option<(u64, u64)> = None;
    let mut ranges: Option<(u64, u64)> = None;
    let mut rnglists_base: Option<u64> = None;
    let mut addrx: &[u8] = &[];

    // Read all interesting debug records.
    loop {
        let name = abbrev.uleb();
        let form = abbrev.uleb();
        if name == 0 && form == 0 {
            break;
        }
        let val = read_scalar::<E>(diag, &mut die, form, hdr.offset_size);
        match name as u32 {
            DW_AT_low_pc => low_pc = Some((form, val)),
            DW_AT_high_pc => high_pc = Some((form, val)),
            DW_AT_rnglists_base => rnglists_base = Some(val),
            DW_AT_addr_base => addrx = &secs.addr[val as usize..],
            DW_AT_ranges => ranges = Some((form, val)),
            _ => {}
        }
    }

    let addr_at = |i: u64| -> u64 {
        Reader::<E>::new(diag, addrx, i as usize * E::WORD_SIZE).uint(E::WORD_SIZE)
    };
    let base = low_pc.map_or(0, |(_, val)| val);

    // Before DWARF 5, DW_AT_ranges is a byte offset into .debug_ranges. In
    // DWARF 5 it is either a direct .debug_rnglists offset (sec_offset) or an
    // index into the offset table rooted at DW_AT_rnglists_base (rnglistx).
    if let Some((form, val)) = ranges {
        if hdr.version <= 4 {
            let mut r = Reader::<E>::new(diag, secs.ranges, val as usize);
            return read_debug_ranges::<E>(&mut r, base);
        }
        if form == DW_FORM_sec_offset as u64 {
            let mut r = Reader::<E>::new(diag, secs.rnglists, val as usize);
            return read_rnglist::<E>(diag, &mut r, addrx, base);
        }
        let Some(list_base) = rnglists_base else {
            fatal!(diag, "--gdb-index: missing DW_AT_rnglists_base");
        };
        let mut entry = Reader::<E>::new(
            diag,
            secs.rnglists,
            (list_base + val * hdr.offset_size as u64) as usize,
        );
        let offset = entry.offset(hdr.offset_size);
        let mut r = Reader::<E>::new(diag, secs.rnglists, (list_base + offset) as usize);
        return read_rnglist::<E>(diag, &mut r, addrx, base);
    }

    // For one contiguous range, high_pc is either an address or an unsigned
    // length, as indicated by its form. DWARF 5 may store either endpoint as an
    // index into the address table rooted at DW_AT_addr_base.
    let (Some((lo_form, lo_val)), Some((hi_form, hi_val))) = (low_pc, high_pc) else {
        return Vec::new();
    };
    let lo = match lo_form as u32 {
        DW_FORM_addr => lo_val,
        DW_FORM_addrx | DW_FORM_addrx1 | DW_FORM_addrx2 | DW_FORM_addrx4 => addr_at(lo_val),
        _ => fatal!(
            diag,
            "--gdb-index: unhandled form for DW_AT_low_pc: {lo_form:#x}"
        ),
    };
    let hi = match hi_form as u32 {
        DW_FORM_addr => hi_val,
        DW_FORM_addrx | DW_FORM_addrx1 | DW_FORM_addrx2 | DW_FORM_addrx4 => addr_at(hi_val),
        DW_FORM_udata | DW_FORM_data1 | DW_FORM_data2 | DW_FORM_data4 | DW_FORM_data8 => {
            lo + hi_val
        }
        _ => fatal!(
            diag,
            "--gdb-index: unhandled form for DW_AT_high_pc: {hi_form:#x}"
        ),
    };
    vec![(lo, hi)]
}

/// CUs and TUs collected from one object file or from the entire link.
#[derive(Default)]
struct FileUnits {
    cus: Vec<Compunit>,
    tus: Vec<Typeunit>,
}

/// One live `.debug_info` contribution needed by the background reader.
struct DebugInfoInput {
    shndx: u32,
    contents: &'static [u8],
}

/// A relocated `.debug_gnu_pubnames` header field and the unit it names.
struct PubnamesRelocation {
    offset: u64,
    target_shndx: u32,
    unit_offset: u64,
}

/// One GNU pubnames or pubtypes contribution needed by the background reader.
struct PubnamesInput {
    contents: &'static [u8],
    relocations: Vec<PubnamesRelocation>,
}

/// Immutable input owned by the background `.gdb_index` reader. The section
/// contents are either input mappings or leaked decompression buffers, so the
/// foreground passes may continue mutating their ObjectFiles independently.
pub struct GdbInputFile {
    file: u32,
    name: String,
    debug_info: Vec<DebugInfoInput>,
    pubnames: Vec<PubnamesInput>,
}

/// Prepares immutable views of the debug sections before foreground and
/// `.gdb_index` work diverge. Relocations are reduced to the unit associations
/// the reader needs, so the background task never aliases an ObjectFile.
pub fn prepare_inputs<E: Arch>(ctx: &mut Context<E>) -> Vec<GdbInputFile> {
    let diag = &*ctx.diag;
    ctx.objs
        .par_iter_mut()
        .map(|file| {
            let file_id = file.id().0;
            let name = file.to_string();
            let mut debug_info = Vec::new();
            for shndx in file.debug_info_sections.clone() {
                let Some(section_name) = file.section(shndx as usize).map(|isec| isec.name(file))
                else {
                    continue;
                };
                let input_size = file.base.shdrs.sh_offset_and_size(shndx as usize).1 as usize;
                let Some(isec) = file.section_mut(shndx as usize) else {
                    continue;
                };

                // Skip type units that lost COMDAT group selection.
                if !isec.is_alive() {
                    continue;
                }
                isec.uncompress::<E>(diag, &name, section_name, input_size);
                debug_info.push(DebugInfoInput {
                    shndx,
                    contents: isec.contents(),
                });
            }

            let mut pubnames = Vec::new();
            for shndx in [file.debug_pubnames, file.debug_pubtypes]
                .into_iter()
                .flatten()
            {
                let Some(section_name) = file.section(shndx as usize).map(|isec| isec.name(file))
                else {
                    continue;
                };
                let input_size = file.base.shdrs.sh_offset_and_size(shndx as usize).1 as usize;
                let Some(isec) = file.section_mut(shndx as usize) else {
                    continue;
                };
                isec.uncompress::<E>(diag, &name, section_name, input_size);

                let isec = file.section_at(shndx);
                let mut relocations = Vec::new();
                file.for_each_relocation::<E>(diag, isec.relsec_idx(), |rel, _| {
                    let esym = file.base.elf_syms.at(rel.r_sym as usize);
                    if let Some(target) = file.symbol_section(rel.r_sym as usize) {
                        relocations.push(PubnamesRelocation {
                            offset: rel.r_offset,
                            target_shndx: target.shndx,
                            unit_offset: esym
                                .st_value
                                .wrapping_add(isec.rel_addend::<E>(&rel) as u64),
                        });
                    }
                });
                pubnames.push(PubnamesInput {
                    contents: isec.contents(),
                    relocations,
                });
            }

            GdbInputFile {
                file: file_id,
                name,
                debug_info,
                pubnames,
            }
        })
        .collect()
}

/// Reads the units of every live `.debug_info` section of a file.
fn read_debug_units<E: Arch>(diag: &Diagnostics, file: &GdbInputFile, file_idx: u32) -> FileUnits {
    let mut units = FileUnits::default();
    for input in &file.debug_info {
        // Read every unit in one input .debug_info contribution. Keeping this separate
        // leaves read_debug_units responsible only for object-level orchestration.
        let contents = input.contents;
        let mut pos = 0;
        while pos < contents.len() {
            let unit = parse_unit_header::<E>(diag, contents, pos);
            match unit.unit_type as u32 {
                DW_UT_compile | DW_UT_partial | DW_UT_skeleton | DW_UT_split_compile => {
                    units.cus.push(Compunit {
                        offset: pos as u64,
                        size: unit.size,
                        file: file_idx,
                        shndx: input.shndx,
                        names: Vec::new(),
                        ranges: Vec::new(),
                    })
                }
                DW_UT_type | DW_UT_split_type => units.tus.push(Typeunit {
                    offset: pos as u64,
                    type_die_offset: unit.type_die_offset,
                    signature: unit.signature,
                    file: file_idx,
                    shndx: input.shndx,
                    names: Vec::new(),
                }),
                kind => fatal!(diag, "--gdb-index: unknown unit type: {kind:#x}"),
            }
            pos += unit.size as usize;
        }
    }
    units
}

/// Returns the .debug_info contribution a pubnames set refers to. The set
/// header's debug_info_offset field is relocated against the particular
/// contribution containing the unit. This matters for DWARF 5 because type
/// units live in separate COMDAT contributions with the same section name.
fn pubnames_unit<'a>(
    input: &PubnamesInput,
    field_offset: u64,
    units: &'a mut FileUnits,
) -> Option<&'a mut Vec<NameRecord>> {
    let rel = input
        .relocations
        .get(
            input
                .relocations
                .partition_point(|r| r.offset < field_offset),
        )
        .filter(|r| r.offset == field_offset)?;
    let key = (rel.target_shndx, rel.unit_offset);

    // Units are appended in input section and contribution offset order, so both
    // the CU and TU vectors are sorted by this key.
    if let Ok(i) = units
        .cus
        .binary_search_by_key(&key, |cu| (cu.shndx, cu.offset))
    {
        return Some(&mut units.cus[i].names);
    }
    if let Ok(i) = units
        .tus
        .binary_search_by_key(&key, |tu| (tu.shndx, tu.offset))
    {
        return Some(&mut units.tus[i].names);
    }
    None
}

/// Parses .debug_gnu_pubnames and .debug_gnu_pubtypes. Each set starts with a
/// DWARF32 or DWARF64 header identifying one debug unit, followed by
/// (DIE offset, 1-byte kind, NUL-terminated name) tuples. The GNU kind byte lets
/// GDB distinguish functions, variables and types without reading their DIEs.
fn read_pubnames<E: Arch>(diag: &Diagnostics, file: &GdbInputFile, units: &mut FileUnits) {
    for input in &file.pubnames {
        let contents = input.contents;
        let mut pos = 0;
        while pos < contents.len() {
            let mut r = Reader::<E>::new(diag, contents, pos);
            let (set_size, offset_size, field_offset) = if r.u32() == u32::MAX {
                // Header of one GNU pubnames or pubtypes set in DWARF64 format.
                let size = r.u64();
                (size + 12, 8, pos as u64 + 14)
            } else {
                // Header of one GNU pubnames or pubtypes set in DWARF32 format.
                r.pos = pos;
                let size = r.u32() as u64;
                (size + 4, 4, pos as u64 + 6)
            };
            r.u16(); // version
            r.offset(offset_size); // debug_info_offset
            r.offset(offset_size); // debug_info_size

            let Some(names) = pubnames_unit(input, field_offset, units) else {
                fatal!(diag, "{}: corrupted debug_info_offset", file.name);
            };
            let end = pos + set_size as usize;
            while r.pos < end {
                if r.offset(offset_size) == 0 {
                    break;
                }
                let kind = r.u8();
                let name = r.cstr();
                names.push(NameRecord::new(
                    xxhash_rust::xxh3::xxh3_64(name),
                    kind,
                    name,
                ));
            }
            pos = end;
        }
    }
}

/// GCC can emit the same public name once for each COMDAT group. Remove these
/// duplicates with a local hash table instead of sorting the strings.
fn dedup_names(names: &mut Vec<NameRecord>) {
    if names.len() < 2 {
        return;
    }

    let capacity = (names.len() * 2).next_power_of_two();
    let mut buckets = vec![u32::MAX; capacity];
    let mask = capacity - 1;
    let mut out = 0;

    for i in 0..names.len() {
        let nametype = names[i].nametype();
        let mut idx = (nametype.hash() ^ (nametype.kind() as u64 * 0x9e37_79b9)) as usize & mask;
        let mut duplicate = false;

        while buckets[idx] != u32::MAX {
            if names[buckets[idx] as usize].nametype().same_name(nametype) {
                duplicate = true;
                break;
            }
            idx = (idx + 1) & mask;
        }

        if duplicate {
            continue;
        }
        if i != out {
            names[out] = names[i];
        }
        buckets[idx] = out as u32;
        out += 1;
    }
    names.truncate(out);
}

/// Compute the .gdb_index hash and length together when a name is first
/// inserted.
fn initialize_gdb_name(name: *const u8) -> (u32, NameEntry) {
    let mut hash = 0u32;
    let mut size = 0u32;
    loop {
        // SAFETY: NameType points to a NUL-terminated public name.
        let mut c = unsafe { *name.add(size as usize) };
        if c == 0 {
            break;
        }
        if c.is_ascii_uppercase() {
            c = c.to_ascii_lowercase();
        }
        hash = hash
            .wrapping_mul(67)
            .wrapping_add(c as u32)
            .wrapping_sub(113);
        size += 1;
    }
    (
        size,
        NameEntry {
            gdb_hash: hash,
            count: AtomicU32::new(0),
            type_vector_offset: 0,
            name_offset: 0,
        },
    )
}

// GDB index type lists contain fixed-width 32-bit records. Four byte-wise
// passes are faster than comparison sorting once a list is sufficiently large.
fn radix_sort(values: &mut [u32], scratch: &mut Vec<u32>) {
    scratch.resize(values.len(), 0);
    let mut in_values = true;

    for shift in (0..32).step_by(8) {
        let mut counts = [0usize; 256];
        let src = if in_values { &*values } else { &*scratch };
        for &value in src {
            counts[(value >> shift) as usize & 255] += 1;
        }

        let mut offsets = [0usize; 256];
        for i in 1..256 {
            offsets[i] = offsets[i - 1] + counts[i - 1];
        }

        if in_values {
            for &value in &*values {
                let bucket = (value >> shift) as usize & 255;
                scratch[offsets[bucket]] = value;
                offsets[bucket] += 1;
            }
        } else {
            for &value in &*scratch {
                let bucket = (value >> shift) as usize & 255;
                values[offsets[bucket]] = value;
                offsets[bucket] += 1;
            }
        }
        in_values = !in_values;
    }

    debug_assert!(in_values);
}

fn estimate_names<T: Sync>(units: &[T], names: impl Fn(&T) -> &[NameRecord] + Sync) -> HyperLogLog {
    units
        .par_iter()
        .fold(HyperLogLog::default, |mut sketch, unit| {
            // NameType keeps 56 hash bits. Spread them across a 64-bit word because
            // HyperLogLog uses the number of leading zero bits.
            for record in names(unit) {
                sketch.insert(record.nametype().hash().wrapping_mul(0x9e37_79b9_7f4a_7c15));
            }
            sketch
        })
        .reduce(HyperLogLog::default, |a, b| a.merged(&b))
}

/// Read compilation units and their public names, deduplicate and intern the
/// names, and determine the constant-pool layout. This stage needs only input
/// sections, so it can run before output-section offsets are assigned.
pub fn read_inputs<E: Arch>(
    timer: Timer,
    diag: &Diagnostics,
    files: Vec<GdbInputFile>,
) -> GdbIndexData {
    let _timer = timer;
    let per_file: Vec<FileUnits> = files
        .par_iter()
        .map(|file| {
            let mut units = read_debug_units::<E>(diag, file, file.file);
            read_pubnames::<E>(diag, file, &mut units);
            for cu in &mut units.cus {
                dedup_names(&mut cu.names);
            }
            for tu in &mut units.tus {
                dedup_names(&mut tu.names);
            }
            units
        })
        .collect();

    let num_cus = per_file.iter().map(|units| units.cus.len()).sum();
    let num_tus = per_file.iter().map(|units| units.tus.len()).sum();
    let mut cus = Vec::with_capacity(num_cus);
    let mut tus = Vec::with_capacity(num_tus);
    for mut units in per_file {
        cus.append(&mut units.cus);
        tus.append(&mut units.tus);
    }

    let mut estimate = estimate_names(&cus, |cu| &cu.names);
    estimate = estimate.merged(&estimate_names(&tus, |tu| &tu.names));
    let map: ConcurrentMap<NameEntry> =
        ConcurrentMap::with_capacity(estimate.cardinality() as usize * 3 / 2);
    let intern = |names: &mut [NameRecord]| {
        for record in names {
            let nametype = record.nametype();
            // SAFETY: NameType stores a NUL-terminated string that remains
            // live for the complete link.
            let (_, value, _) = unsafe {
                map.insert_cstr_with(
                    nametype.name as *const u8,
                    nametype.hash(),
                    initialize_gdb_name,
                )
            };
            record.set_indexed(IndexedName {
                entry: NameEntryRef::new(value),
                type_vector_idx: value.count.fetch_add(1, Ordering::Relaxed) + 1,
                kind: nametype.kind(),
            });
        }
    };
    cus.par_iter_mut().for_each(|cu| intern(&mut cu.names));
    tus.par_iter_mut().for_each(|tu| intern(&mut tu.names));

    // Lay out the constant pool: all type vectors, then all names, in a
    // deterministic order.
    let entries = map.sorted_entry_refs_all();
    let names = map.freeze();

    // The map may contain millions of names. Assign their type and string
    // ranges with a parallel prefix sum.
    let chunk_size = entries.len().div_ceil(rayon::current_num_threads()).max(1);
    let chunk_sizes: Vec<PoolSize> = entries
        .par_chunks(chunk_size)
        .map(|chunk| {
            let mut size = PoolSize::default();
            for &entry_ref in chunk {
                let entry = entry_ref.value(&names);
                size.type_bytes += entry.count.load(Ordering::Relaxed) * 4 + 4;
                size.name_bytes += entry_ref.key_len(&names) as u32 + 1;
            }
            size
        })
        .collect();
    let mut pool_size = PoolSize::default();
    let chunk_offsets: Vec<PoolSize> = chunk_sizes
        .iter()
        .map(|&size| {
            let offset = pool_size;
            pool_size.type_bytes += size.type_bytes;
            pool_size.name_bytes += size.name_bytes;
            offset
        })
        .collect();
    entries
        .par_chunks(chunk_size)
        .zip(chunk_offsets)
        .for_each(|(chunk, mut size)| {
            for &entry_ref in chunk {
                // SAFETY: entries contains each map entry exactly once, so
                // parallel chunks update disjoint values.
                let entry = unsafe { &mut *entry_ref.value_mut_ptr(&names) };
                entry.type_vector_offset = size.type_bytes;
                entry.name_offset = size.name_bytes;
                size.type_bytes += entry.count.load(Ordering::Relaxed) * 4 + 4;
                size.name_bytes += entry_ref.key_len(&names) as u32 + 1;
            }
        });
    entries.par_iter().for_each(|&entry_ref| {
        // SAFETY: as above, each entry appears once.
        unsafe { &mut *entry_ref.value_mut_ptr(&names) }.name_offset += pool_size.type_bytes;
    });

    let ht_size = (entries.len() as u32 * 5 / 4 + 1).next_power_of_two();
    GdbIndexData {
        cus,
        tus,
        names: Some(names),
        entries,
        type_pool_size: pool_size.type_bytes,
        name_pool_size: pool_size.name_bytes,
        ht_size,
        tables: Vec::new(),
    }
}

/// Unit offsets are relative to their input .debug_info contributions.
/// Convert them to output-section offsets and sort each .gdb_index list in
/// output order. The format's unit-number namespace consists of every CU-list
/// entry followed by every TU-list entry, even when CUs and TUs are
/// interleaved in .debug_info. Address-area records refer only to the CU list.
///
/// The Rust port performs the sorting in `build_tables` after this rebasing.
pub fn prepare_tables<E: Arch>(ctx: &Context<E>, data: &mut GdbIndexData) {
    let output_offset = |file: u32, shndx: u32| ctx.objs[file as usize].section_at(shndx).offset();
    for cu in &mut data.cus {
        cu.offset += output_offset(cu.file, cu.shndx);
    }
    for tu in &mut data.tus {
        tu.offset += output_offset(tu.file, tu.shndx);
    }
}

fn limited_parallel_for_mut<T: Send>(
    values: &mut [T],
    workers: usize,
    op: impl Fn(usize, &mut T) + Send + Sync,
) {
    limited_parallel_for_mut_init(values, workers, || (), |_, i, value| op(i, value));
}

fn limited_parallel_for_mut_init<T: Send, S: Send>(
    values: &mut [T],
    workers: usize,
    init: impl Fn() -> S + Send + Sync,
    op: impl Fn(&mut S, usize, &mut T) + Send + Sync,
) {
    if values.is_empty() {
        return;
    }

    let len = values.len();
    let workers = workers.max(1).min(len);
    let chunk_size = len.div_ceil(workers.saturating_mul(2)).max(1);
    let next = AtomicUsize::new(0);
    let addr = values.as_mut_ptr() as usize;
    (0..workers).into_par_iter().for_each(|_| {
        let mut state = init();
        loop {
            let begin = next.fetch_add(chunk_size, Ordering::Relaxed);
            if begin >= len {
                break;
            }
            let end = (begin + chunk_size).min(len);
            for i in begin..end {
                // SAFETY: every range is returned by one fetch_add, so parallel
                // workers receive disjoint elements of values.
                op(&mut state, i, unsafe { &mut *(addr as *mut T).add(i) });
            }
        }
    });
}

/// Build the name lookup table and the constant pool for .gdb_index. They
/// depend on compilation-unit order but not on relocated address ranges, so
/// they can be built in the background once .debug_info offsets are fixed.
pub fn build_tables(timer: Timer, mut data: GdbIndexData, workers: usize) -> GdbIndexData {
    let _timer = timer;
    if data.cus.is_empty() && data.tus.is_empty() {
        return data;
    }

    // Units are numbered by their position in the output: all
    // compilation units, then all type units.
    data.cus.sort_by_key(|cu| cu.offset);
    data.tus.sort_by_key(|tu| tu.offset);

    let names = data.names.as_ref().unwrap();
    let symtab_size = data.ht_size as usize * 8;
    let pool_size = (data.type_pool_size + data.name_pool_size) as usize;
    let table_size = symtab_size + pool_size;
    let table_words = table_size.div_ceil(4);
    let mut tables = Vec::<std::mem::MaybeUninit<u32>>::with_capacity(table_words);
    // SAFETY: MaybeUninit may be left uninitialized. Every serialized byte is
    // written below before the storage is converted to u32 words.
    unsafe { tables.set_len(table_words) };
    let table_addr = tables.as_mut_ptr() as usize;

    // `tables` contains the name hash table followed by the constant pool. The
    // constant pool contains all type vectors followed by all name strings.
    // Each occupied hash-table slot contains the constant-pool offsets of a
    // name and its type vector.
    // SAFETY: table storage has at least symtab_size bytes. Initializing them
    // before making the u32 slice avoids forming references to uninitialized
    // integers.
    unsafe { std::ptr::write_bytes(table_addr as *mut u8, 0, symtab_size) };
    let ht = unsafe { std::slice::from_raw_parts_mut(table_addr as *mut u32, symtab_size / 4) };

    // This probing sequence is part of the .gdb_index format. The table size
    // is a power of two, so an odd step visits every slot.
    let mask = data.ht_size - 1;
    for &entry_ref in &data.entries {
        let entry = entry_ref.value(names);
        let step = ((entry.gdb_hash.wrapping_mul(17)) & mask) | 1;
        let mut i = entry.gdb_hash & mask;
        while ht[i as usize * 2] != 0 || ht[i as usize * 2 + 1] != 0 {
            i = (i + step) & mask;
        }
        ht[i as usize * 2] = entry.name_offset.to_le();
        ht[i as usize * 2 + 1] = entry.type_vector_offset.to_le();
    }

    let pool_addr = table_addr + symtab_size;

    // Each occurrence of a name contributes one value to its type vector. Each
    // occurrence was assigned a distinct slot while the names were interned, so
    // the vectors can be filled in parallel. The high byte is the name's type
    // and the low 24 bits are the unit number. TU numbers follow CU numbers.
    let write_names = |records: &[NameRecord], unit: usize| {
        for record in records {
            let name = record.indexed();
            let entry = name.entry.get(names);
            let offset = entry.type_vector_offset as usize + name.type_vector_idx as usize * 4;
            debug_assert!(offset + 4 <= data.type_pool_size as usize);
            let value = (name.kind as u32) << 24 | unit as u32;
            // SAFETY: the pool starts on a u32 boundary, all offsets are
            // multiples of four, and every occurrence reserved a distinct
            // slot before this parallel phase.
            unsafe { (pool_addr as *mut u32).add(offset / 4).write(value) };
        }
    };
    let num_cus = data.cus.len();
    limited_parallel_for_mut(&mut data.cus, workers, |i, cu| {
        write_names(&cu.names, i);
    });
    limited_parallel_for_mut(&mut data.tus, workers, |i, tu| {
        write_names(&tu.names, num_cus + i);
    });

    // Prefix each type vector with its length and sort it for deterministic
    // output. Store the NUL-terminated name at its assigned string-pool offset.
    limited_parallel_for_mut_init(
        &mut data.entries,
        workers,
        Vec::new,
        |scratch, _, entry_ref| {
            let entry_ref = *entry_ref;
            let entry = entry_ref.value(names);
            let count = entry.count.load(Ordering::Relaxed);
            let words = (pool_addr as *mut u32).wrapping_add(entry.type_vector_offset as usize / 4);
            // SAFETY: every occurrence filled its distinct value slot above;
            // writing the prefix completes this vector before a slice is made.
            unsafe { words.write(count) };
            let values = unsafe { std::slice::from_raw_parts_mut(words.add(1), count as usize) };
            if values.len() < 256 {
                values.sort_unstable();
            } else {
                radix_sort(values, scratch);
            }
            // SAFETY: the prefix and all count values are now initialized.
            let words = unsafe { std::slice::from_raw_parts_mut(words, count as usize + 1) };
            for word in &mut *words {
                *word = word.to_le();
            }
            let key = entry_ref.key(names);
            let name = (pool_addr as *mut u8).wrapping_add(entry.name_offset as usize);
            // SAFETY: prefix-scan offsets assign this entry a distinct
            // key.len()+1 byte range in the name pool.
            unsafe {
                name.copy_from_nonoverlapping(key.as_ptr(), key.len());
                name.add(key.len()).write(0);
            }
        },
    );

    // Vec<u32> rounds the byte allocation up to a whole word; initialize only
    // those padding bytes, which are not part of the serialized tables.
    let padded_size = table_words * 4;
    for i in table_size..padded_size {
        // SAFETY: these are the allocation's final padding bytes and no worker
        // remains active.
        unsafe { (table_addr as *mut u8).add(i).write(0) };
    }
    let (table_ptr, table_len, table_capacity) = (
        tables.as_mut_ptr().cast::<u32>(),
        tables.len(),
        tables.capacity(),
    );
    std::mem::forget(tables);
    // SAFETY: the hash table, every type vector, every name including its NUL,
    // and the final allocation padding have all been initialized. u32 and
    // MaybeUninit<u32> have identical allocation layouts.
    let tables = unsafe { Vec::from_raw_parts(table_ptr, table_len, table_capacity) };

    // The serialized tables contain everything needed from names and the map.
    // Release their storage here so reclamation remains part of this background
    // phase rather than delaying the final output path.
    limited_parallel_for_mut(&mut data.cus, workers, |_, cu| {
        cu.names = Vec::new();
    });
    limited_parallel_for_mut(&mut data.tus, workers, |_, tu| {
        tu.names = Vec::new();
    });
    data.entries = Vec::new();
    data.names = None;
    data.tables = tables;
    data
}

/// Builds the tables synchronously for the separate-debug-file path, where
/// the output's section ordering differs from the main file.
pub fn build_tables_now<E: Arch>(ctx: &mut Context<E>) {
    let timer = ctx.timer("build_gdb_index_tables");
    let Some(mut data) = ctx.gdb_index_data.take() else {
        return;
    };
    prepare_tables(ctx, &mut data);
    ctx.gdb_index_data = Some(build_tables(timer, data, rayon::current_num_threads()));
}

/// The contents of a debug section in the output, or its uncompressed
/// contents if it was compressed.
fn section_contents<'a, E: Arch>(ctx: &'a Context<E>, buf: &'a [u8], name: &str) -> &'a [u8] {
    for &id in &ctx.chunks {
        let hdr = ctx.chunk_header(id);
        if hdr.name != name {
            continue;
        }
        if let ChunkId::Compressed(i) = id {
            return ctx.compressed_sections[i as usize]
                .uncompressed_data
                .as_deref()
                .unwrap_or(&[]);
        }
        return &buf[hdr.shdr.sh_offset as usize..(hdr.shdr.sh_offset + hdr.shdr.sh_size) as usize];
    }
    &[]
}

fn parallel_copy(dst: &mut [u8], src: &[u8]) {
    const BLOCK_SIZE: usize = 2 * 1024 * 1024;

    debug_assert_eq!(dst.len(), src.len());
    dst.par_chunks_mut(BLOCK_SIZE)
        .zip(src.par_chunks(BLOCK_SIZE))
        .for_each(|(dst, src)| dst.copy_from_slice(src));
}

/// Read relocated address ranges and serialize the index prepared above.
pub fn write<E: Arch>(ctx: &mut Context<E>, output: &mut OutputFile) {
    let _t = ctx.timer("write_gdb_index");
    let Some(mut data) = ctx.gdb_index_data.take() else {
        return;
    };
    if data.cus.is_empty() && data.tus.is_empty() {
        return;
    }

    // Find debug info sections
    {
        let buf = output.buf();
        let secs = RangeSections {
            info: section_contents(ctx, buf, ".debug_info"),
            abbrev: section_contents(ctx, buf, ".debug_abbrev"),
            ranges: section_contents(ctx, buf, ".debug_ranges"),
            addr: section_contents(ctx, buf, ".debug_addr"),
            rnglists: section_contents(ctx, buf, ".debug_rnglists"),
        };
        let diag = &ctx.diag;
        data.cus.par_iter_mut().for_each(|cu| {
            cu.ranges = read_address_ranges::<E>(diag, &secs, cu);
            cu.ranges.retain(|&(start, end)| start != 0 && start != end);
        });
    }

    // Version 8 made symbol-table entries refer directly to type units, as ours
    // do. GDB 12 accepts version 8 but assumes that its type-unit list refers to
    // .debug_types and crashes on DWARF 5 type units in .debug_info. Version 9
    // makes it safely ignore such an index, while newer GDB versions can use it.
    // Keep version 7 for TU-free indices so older GDB versions can still use them.
    let has_tus = !data.tus.is_empty();
    // The version 7 .gdb_index header. All table offsets are section-relative.
    //
    // Version 9 inserts a shortcut table between the symbol table and the
    // constant pool. We currently emit an empty shortcut table.
    let header_size = if has_tus { 28 } else { 24 };
    // Compute sizes of each component.
    let cu_list_offset = header_size;
    let cu_types_offset = cu_list_offset + data.cus.len() * 16;
    let ranges_offset = cu_types_offset + data.tus.len() * 24;
    let mut range_offsets = Vec::with_capacity(data.cus.len());
    let mut ranges_size = 0usize;
    for cu in &data.cus {
        range_offsets.push(ranges_size as u64);
        ranges_size += cu.ranges.len() * 20;
    }
    let symtab_offset = ranges_offset + ranges_size;
    let shortcut_offset = symtab_offset + data.ht_size as usize * 8;
    let const_pool_offset = shortcut_offset + if has_tus { 8 } else { 0 };
    let size = const_pool_offset + (data.type_pool_size + data.name_pool_size) as usize;

    let file_size = output.len();
    output.extend(&ctx.diag, size);
    let buf = &mut output.buf()[file_size..];

    // Write a section header. A zero language marks the version 9 shortcut
    // table as containing no main-function information.
    let mut header: Vec<u32> = vec![
        if has_tus { 9 } else { 7 },
        cu_list_offset as u32,
        cu_types_offset as u32,
        ranges_offset as u32,
        symtab_offset as u32,
    ];
    if has_tus {
        header.push(shortcut_offset as u32);
    }
    header.push(const_pool_offset as u32);
    for (i, word) in header.iter().enumerate() {
        buf[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }

    // A CU-list entry is {.debug_info offset, unit size}.
    let mut p = cu_list_offset;
    for cu in &data.cus {
        buf[p..p + 8].copy_from_slice(&cu.offset.to_le_bytes());
        buf[p + 8..p + 16].copy_from_slice(&cu.size.to_le_bytes());
        p += 16;
    }
    // A TU-list entry is {.debug_info offset, unit-relative type DIE offset,
    // signature}. Unlike a CU-list entry, it does not contain the unit size.
    for tu in &data.tus {
        buf[p..p + 8].copy_from_slice(&tu.offset.to_le_bytes());
        buf[p + 8..p + 16].copy_from_slice(&tu.type_die_offset.to_le_bytes());
        buf[p + 16..p + 24].copy_from_slice(&tu.signature.to_le_bytes());
        p += 24;
    }

    // An address-area entry is {start address, end address, CU-list index}.
    data.cus
        .par_iter()
        .enumerate()
        .zip(split_at_offsets(
            &mut buf[ranges_offset..symtab_offset],
            &range_offsets,
        ))
        .for_each(|((i, cu), buf)| {
            for (range, entry) in cu.ranges.iter().zip(buf.chunks_exact_mut(20)) {
                entry[..8].copy_from_slice(&range.0.to_le_bytes());
                entry[8..16].copy_from_slice(&range.1.to_le_bytes());
                entry[16..20].copy_from_slice(&(i as u32).to_le_bytes());
            }
        });

    let symtab_size = data.ht_size as usize * 8;
    let tables = words_as_bytes(&data.tables);
    parallel_copy(
        &mut buf[symtab_offset..symtab_offset + symtab_size],
        &tables[..symtab_size],
    );
    parallel_copy(
        &mut buf[const_pool_offset..size],
        &tables[symtab_size..symtab_size + size - const_pool_offset],
    );

    // Update the section size and rewrite the section header
    if let Some(gdb_index) = &mut ctx.gdb_index {
        gdb_index.hdr.shdr.sh_size = size as u64;
    }
    if let Some(section_headers) = &ctx.shdr {
        let shdr = section_headers.hdr.shdr;
        let buf = output.buf();
        crate::output_chunks::copy_buf(
            ctx,
            ChunkId::Shdr,
            &mut buf[shdr.sh_offset as usize..(shdr.sh_offset + shdr.sh_size) as usize],
        );
    }
}
