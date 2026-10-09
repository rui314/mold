//! Lays out the live chunks in output sections, applies the x86_64 COFF
//! relocations, builds the base relocation table and writes the PE headers.
//! The image is built in memory and returned as bytes.

use std::collections::HashMap;
use std::rc::Rc;

use mold_common::fatal;
use mold_common::util::align_to;

use crate::coff::{self, SCN_CNT_CODE, SCN_CNT_INITIALIZED_DATA, SCN_CNT_UNINITIALIZED_DATA};
use crate::link::{Linker, Loc};

/// The image base that lld uses for executables, so that 32-bit absolute
/// addresses (`IMAGE_REL_AMD64_ADDR32`) come out as lld writes them.
pub const DEFAULT_IMAGE_BASE: u64 = 0x1_4000_0000;

const SECTION_ALIGN: u64 = 0x1000;
const FILE_ALIGN: u64 = 0x200;

/// The file offset of the PE signature. Like lld, we leave room for a DOS stub.
const PE_OFFSET: usize = 0x78;
const COFF_HEADER_SIZE: usize = 20;
const OPTIONAL_HEADER_SIZE: usize = 240;
const SECTION_HEADER_SIZE: usize = 40;
const NUM_DATA_DIRECTORIES: usize = 16;
const BASE_RELOC_DIRECTORY: usize = 5;

const RELOC_SECTION_CHARS: u32 = 0x4200_0040;

/// Output sections that lld creates before any others, in its order.
const BUILTIN_SECTIONS: &[&[u8]] = &[
    b".text",
    b".rdata",
    b".buildid",
    b".cvinfo",
    b".data",
    b".pdata",
    b".idata",
    b".edata",
    b".didat",
    b".rsrc",
    b".reloc",
    b".ctors",
    b".dtors",
];

const IMAGE_REL_AMD64_ABSOLUTE: u16 = 0;
const IMAGE_REL_AMD64_ADDR64: u16 = 1;
const IMAGE_REL_AMD64_ADDR32: u16 = 2;
const IMAGE_REL_AMD64_ADDR32NB: u16 = 3;
const IMAGE_REL_AMD64_REL32: u16 = 4;
const IMAGE_REL_AMD64_REL32_5: u16 = 9;

const IMAGE_REL_BASED_HIGHLOW: u8 = 3;
const IMAGE_REL_BASED_DIR64: u8 = 10;

const IMAGE_FILE_EXECUTABLE_IMAGE: u16 = 0x0002;
const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x0020;
const IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA: u16 = 0x0020;
const IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE: u16 = 0x0040;
const IMAGE_DLLCHARACTERISTICS_NX_COMPAT: u16 = 0x0100;
const IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE: u16 = 0x8000;

const OUTPUT_CHAR_MASK: u32 = SCN_CNT_CODE
    | SCN_CNT_INITIALIZED_DATA
    | SCN_CNT_UNINITIALIZED_DATA
    | coff::SCN_MEM_READ
    | coff::SCN_MEM_WRITE
    | coff::SCN_MEM_EXECUTE;

/// Where a relocation points: an RVA in the image, or an absolute value.
#[derive(Clone, Copy)]
enum Target {
    Image(u64),
    Abs(u64),
}

/// A member of an output section: a chunk, with the keys that order it.
struct Member {
    uninit: bool,
    /// The rank of the chunk's partial section: the chunks that share a full
    /// input section name and characteristics. lld orders partial sections
    /// by that key.
    partial: u32,
    /// The position of the chunk in lld's order: by object completion, then section.
    seq: u32,
    chunk: u32,
}

/// An output section: the chunks that share a name, laid out in order.
struct OutSection {
    name: Vec<u8>,
    chars: u32,
    members: Vec<Member>,
    init_size: u64,
    virt_size: u64,
    rva: u64,
    raw_off: u64,
    raw_size: u64,
}

/// Lays out the live chunks of `ln` and returns the image.
pub(crate) fn build(ln: &mut Linker<'_>, entry: u32) -> Vec<u8> {
    let image_base = ln.opts.image_base.unwrap_or(DEFAULT_IMAGE_BASE);

    let mut outs = group_chunks(ln);
    sort_outs(&mut outs);
    for out in &mut outs {
        // Initialized chunks come first. Uninitialized ones, which are
        // merged into the same output section, follow them.
        out.members.sort_by_key(|m| (m.uninit, m.partial, m.seq));
        let mut cursor = 0u64;
        let mut init_size = 0u64;
        for m in &out.members {
            let ch = &mut ln.chunks[m.chunk as usize];
            cursor = align_to(cursor, ch.align as u64);
            ch.offset = cursor as u32;
            cursor += ch.size as u64;
            if !m.uninit {
                init_size = cursor;
            }
        }
        out.init_size = init_size;
        out.virt_size = cursor;
    }

    let ln_ref: &Linker<'_> = ln;
    let have_relocs = outs
        .iter()
        .flat_map(|o| o.members.iter())
        .any(|m| has_address_relocs(ln_ref, m.chunk as usize));
    let emitted = outs.iter().filter(|o| o.virt_size > 0).count();
    let nsec = emitted + usize::from(have_relocs);
    let headers_size = align_to(
        (PE_OFFSET + 4 + COFF_HEADER_SIZE + OPTIONAL_HEADER_SIZE + SECTION_HEADER_SIZE * nsec)
            as u64,
        FILE_ALIGN,
    );

    let mut rva = align_to(headers_size, SECTION_ALIGN);
    let mut file = headers_size;
    for out in &mut outs {
        out.rva = rva;
        if out.init_size > 0 {
            out.raw_size = align_to(out.init_size, FILE_ALIGN);
            out.raw_off = file;
            file += out.raw_size;
        }
        rva += align_to(out.virt_size, SECTION_ALIGN);
    }

    // The RVA and file offset of every chunk that the image keeps.
    let mut chunk_rva = vec![0u64; ln.chunks.len()];
    let mut chunk_file = vec![0u64; ln.chunks.len()];
    for out in &outs {
        for m in &out.members {
            let offset = ln.chunks[m.chunk as usize].offset as u64;
            chunk_rva[m.chunk as usize] = out.rva + offset;
            chunk_file[m.chunk as usize] = out.raw_off + offset;
        }
    }

    let mut image = vec![0u8; file as usize];
    // Like lld, pad code with int3, so that the gaps between functions hold it.
    for out in outs.iter().filter(|o| o.chars & SCN_CNT_CODE != 0) {
        let at = out.raw_off as usize;
        image[at..at + out.raw_size as usize].fill(0xcc);
    }
    for out in &outs {
        for m in &out.members {
            if m.uninit {
                continue;
            }
            let ch = &ln.chunks[m.chunk as usize];
            let obj = Rc::clone(&ln.objs[ch.obj as usize]);
            let data = obj.sections[ch.sec as usize].data;
            let at = chunk_file[m.chunk as usize] as usize;
            image[at..at + data.len()].copy_from_slice(data);
        }
    }

    let mut sites = Vec::new();
    for c in 0..ln.chunks.len() {
        if ln.chunks[c].live {
            apply_relocs(ln, c, image_base, &chunk_rva, &chunk_file, &mut image, &mut sites);
        }
    }

    // The base relocation table goes last, after every other section.
    let reloc_table = base_relocs(&mut sites);
    let mut reloc_dir = (0u64, 0u64);
    if !reloc_table.is_empty() {
        let len = reloc_table.len() as u64;
        let raw_size = align_to(len, FILE_ALIGN);
        image.resize((file + raw_size) as usize, 0);
        image[file as usize..(file + len) as usize].copy_from_slice(&reloc_table);
        outs.push(OutSection {
            name: b".reloc".to_vec(),
            chars: RELOC_SECTION_CHARS,
            members: Vec::new(),
            init_size: len,
            virt_size: len,
            rva,
            raw_off: file,
            raw_size,
        });
        reloc_dir = (rva, len);
        rva += align_to(len, SECTION_ALIGN);
    }

    let entry_rva = match ln.resolve(entry) {
        Some(Loc::Chunk { chunk, value }) => chunk_rva[chunk as usize] + value as u64,
        _ => fatal!(
            "entry symbol {} is not defined in a section that is linked",
            String::from_utf8_lossy(ln.globals[entry as usize].name)
        ),
    };

    let code_size: u64 =
        outs.iter().filter(|o| o.chars & SCN_CNT_CODE != 0).map(|o| o.raw_size).sum();
    let init_size: u64 = outs
        .iter()
        .filter(|o| o.chars & SCN_CNT_CODE == 0 && o.chars & SCN_CNT_INITIALIZED_DATA != 0)
        .map(|o| o.raw_size)
        .sum();
    let uninit_size: u64 = outs.iter().map(|o| o.virt_size.saturating_sub(o.raw_size)).sum();
    let base_of_code = outs.iter().find(|o| o.chars & SCN_CNT_CODE != 0).map_or(0, |o| o.rva);

    let headers = Headers {
        image_base,
        entry_rva,
        subsystem: ln.opts.subsystem,
        nxcompat: ln.opts.nxcompat,
        relocatable: !reloc_table.is_empty(),
        code_size,
        init_size,
        uninit_size,
        base_of_code,
        size_of_image: align_to(rva, SECTION_ALIGN),
        size_of_headers: headers_size,
        reloc_dir,
    };
    let sections: Vec<&OutSection> = outs.iter().filter(|o| o.virt_size > 0).collect();
    write_headers(&mut image, &headers, &sections);
    image
}

/// Groups the live chunks by output section name. Input sections whose names
/// differ only after a '$' are merged, and `.bss` goes into `.data`, as in lld.
fn group_chunks(ln: &Linker<'_>) -> Vec<OutSection> {
    // The live chunks in lld's order: by object completion, then by section.
    let mut order: Vec<usize> = (0..ln.chunks.len()).filter(|&c| ln.chunks[c].live).collect();
    order.sort_by_key(|&c| (ln.file_seq[ln.chunks[c].obj as usize], ln.chunks[c].sec));

    // lld keeps its partial sections in a map keyed by the full input section
    // name and characteristics, so they are laid out in that key's order.
    let key_of = |c: usize| -> (Vec<u8>, u32) {
        let ch = &ln.chunks[c];
        let sec = &ln.objs[ch.obj as usize].sections[ch.sec as usize];
        (sec.name.to_vec(), sec.characteristics & OUTPUT_CHAR_MASK)
    };
    let mut keys: Vec<(Vec<u8>, u32)> = order.iter().map(|&c| key_of(c)).collect();
    keys.sort();
    keys.dedup();
    let rank: HashMap<(Vec<u8>, u32), u32> =
        keys.into_iter().enumerate().map(|(i, k)| (k, i as u32)).collect();

    let mut outs: Vec<OutSection> = Vec::new();
    let mut index: HashMap<Vec<u8>, usize> = HashMap::new();
    for (seq, &c) in order.iter().enumerate() {
        let ch = &ln.chunks[c];
        let sec = &ln.objs[ch.obj as usize].sections[ch.sec as usize];
        let name = out_name(sec.name);
        let i = match index.get(&name) {
            Some(&i) => i,
            None => {
                outs.push(OutSection {
                    name: name.clone(),
                    chars: 0,
                    members: Vec::new(),
                    init_size: 0,
                    virt_size: 0,
                    rva: 0,
                    raw_off: 0,
                    raw_size: 0,
                });
                index.insert(name, outs.len() - 1);
                outs.len() - 1
            }
        };
        let key = key_of(c);
        let out = &mut outs[i];
        out.chars |= key.1;
        out.members.push(Member {
            uninit: ch.uninit,
            partial: rank[&key],
            seq: seq as u32,
            chunk: c as u32,
        });
    }
    outs
}

/// Returns the output section name for an input section name.
fn out_name(name: &[u8]) -> Vec<u8> {
    let base = name.split(|&b| b == b'$').next().unwrap_or(name);
    if base == b".bss" { b".data".to_vec() } else { base.to_vec() }
}

/// Orders output sections as lld does: its builtin sections in their creation
/// order, then the others in the order of their partial sections, which is by name.
fn sort_outs(outs: &mut [OutSection]) {
    fn rank(name: &[u8]) -> (usize, Vec<u8>) {
        match BUILTIN_SECTIONS.iter().position(|&b| b == name) {
            Some(i) => (i, Vec::new()),
            None => (BUILTIN_SECTIONS.len(), name.to_vec()),
        }
    }
    outs.sort_by_cached_key(|o| rank(&o.name));
}

/// Returns true if a chunk has a relocation that needs a base relocation entry.
fn has_address_relocs(ln: &Linker<'_>, c: usize) -> bool {
    let ch = &ln.chunks[c];
    let obj = &ln.objs[ch.obj as usize];
    obj.sections[ch.sec as usize]
        .relocs
        .iter()
        .any(|r| r.kind == IMAGE_REL_AMD64_ADDR64 || r.kind == IMAGE_REL_AMD64_ADDR32)
}

/// Applies the relocations of live chunk `c` to `image`. Each absolute
/// address that depends on the image base is recorded in `sites`.
fn apply_relocs(
    ln: &Linker<'_>,
    c: usize,
    image_base: u64,
    chunk_rva: &[u64],
    chunk_file: &[u64],
    image: &mut [u8],
    sites: &mut Vec<(u32, u8)>,
) {
    let ch = &ln.chunks[c];
    let obj = Rc::clone(&ln.objs[ch.obj as usize]);
    let sec = &obj.sections[ch.sec as usize];
    if sec.relocs.is_empty() {
        return;
    }
    if ch.uninit {
        fatal!("{}: relocations in an uninitialized section", obj.name);
    }
    let p_base_rva = chunk_rva[c];
    let p_base_off = chunk_file[c];

    for r in &sec.relocs {
        let p_rva = p_base_rva + r.offset as u64;
        let p_off = (p_base_off + r.offset as u64) as usize;
        let target = target_of(ln, ch.obj, r.symbol, chunk_rva, &obj.name);
        let va = match target {
            Target::Image(rva) => image_base + rva,
            Target::Abs(v) => v,
        };
        match r.kind {
            IMAGE_REL_AMD64_ABSOLUTE => {}
            IMAGE_REL_AMD64_ADDR64 => {
                if let Target::Image(_) = target {
                    sites.push((rva32(p_rva), IMAGE_REL_BASED_DIR64));
                }
                // COFF relocations add to the field, which holds the addend.
                let addend = u64::from_le_bytes(image[p_off..p_off + 8].try_into().unwrap());
                image[p_off..p_off + 8].copy_from_slice(&addend.wrapping_add(va).to_le_bytes());
            }
            IMAGE_REL_AMD64_ADDR32 => {
                if let Target::Image(_) = target {
                    sites.push((rva32(p_rva), IMAGE_REL_BASED_HIGHLOW));
                }
                let addend = u32::from_le_bytes(image[p_off..p_off + 4].try_into().unwrap());
                image[p_off..p_off + 4]
                    .copy_from_slice(&addend.wrapping_add(va as u32).to_le_bytes());
            }
            IMAGE_REL_AMD64_ADDR32NB => {
                let rva = match target {
                    Target::Image(rva) => rva,
                    Target::Abs(v) => v,
                };
                let addend = u32::from_le_bytes(image[p_off..p_off + 4].try_into().unwrap());
                image[p_off..p_off + 4]
                    .copy_from_slice(&addend.wrapping_add(rva as u32).to_le_bytes());
            }
            IMAGE_REL_AMD64_REL32..=IMAGE_REL_AMD64_REL32_5 => {
                // The field holds an addend, which lld and COFF add to the displacement.
                // IMAGE_REL_AMD64_REL32_n add n for the bytes that follow the field.
                let extra = (r.kind - IMAGE_REL_AMD64_REL32) as u64;
                let pc = (image_base + p_rva + 4 + extra) as i64;
                let addend = i32::from_le_bytes(image[p_off..p_off + 4].try_into().unwrap()) as i64;
                let Ok(delta) = i32::try_from(addend + va as i64 - pc) else {
                    fatal!("{}: relocation out of range at offset {}", obj.name, r.offset);
                };
                image[p_off..p_off + 4].copy_from_slice(&delta.to_le_bytes());
            }
            kind => fatal!("{}: unsupported relocation type 0x{kind:x}", obj.name),
        }
    }
}

fn rva32(rva: u64) -> u32 {
    match u32::try_from(rva) {
        Ok(rva) => rva,
        Err(_) => fatal!("image is larger than 4 GiB"),
    }
}

/// Resolves a symbol referenced by a relocation to an address.
fn target_of(ln: &Linker<'_>, oi: u32, symbol: u32, chunk_rva: &[u64], file: &str) -> Target {
    let sym_name =
        || String::from_utf8_lossy(ln.objs[oi as usize].symbols[symbol as usize].name).into_owned();
    let loc = ln.locs[oi as usize][symbol as usize];
    let resolved = match loc {
        Loc::Global(g) => ln.resolve(g),
        other => Some(other),
    };
    match resolved {
        Some(Loc::Chunk { chunk, value }) => {
            let ch = &ln.chunks[chunk as usize];
            if ch.discarded || !ch.live {
                fatal!("{file}: relocation refers to {} in a discarded COMDAT section", sym_name());
            }
            Target::Image(chunk_rva[chunk as usize] + value as u64)
        }
        Some(Loc::Abs(v)) => Target::Abs(v as u64),
        _ => fatal!("{file}: relocation refers to undefined symbol {}", sym_name()),
    }
}

/// Encodes the base relocation table. Entries are grouped by 4 KiB page, and
/// each block is padded to a multiple of 4 bytes.
fn base_relocs(sites: &mut [(u32, u8)]) -> Vec<u8> {
    sites.sort_unstable();
    let mut out = Vec::new();
    let mut i = 0;
    while i < sites.len() {
        let page = sites[i].0 & !0xfff;
        let start = i;
        while i < sites.len() && sites[i].0 & !0xfff == page {
            i += 1;
        }
        let mut entries: Vec<u16> = sites[start..i]
            .iter()
            .map(|&(rva, kind)| ((kind as u16) << 12) | (rva & 0xfff) as u16)
            .collect();
        if entries.len() % 2 == 1 {
            entries.push(0);
        }
        let block_size = 8 + 2 * entries.len() as u32;
        out.extend_from_slice(&page.to_le_bytes());
        out.extend_from_slice(&block_size.to_le_bytes());
        for e in entries {
            out.extend_from_slice(&e.to_le_bytes());
        }
    }
    out
}

struct Headers {
    image_base: u64,
    entry_rva: u64,
    subsystem: u16,
    nxcompat: bool,
    relocatable: bool,
    code_size: u64,
    init_size: u64,
    uninit_size: u64,
    base_of_code: u64,
    size_of_image: u64,
    size_of_headers: u64,
    reloc_dir: (u64, u64),
}

fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
    buf[at..at + bytes.len()].copy_from_slice(bytes);
}

/// Writes the DOS header, the PE and optional headers, and the section table.
fn write_headers(image: &mut [u8], h: &Headers, sections: &[&OutSection]) {
    let coff = PE_OFFSET + 4;
    let opt = coff + COFF_HEADER_SIZE;
    let table = opt + OPTIONAL_HEADER_SIZE;

    put(image, 0, b"MZ");
    put(image, 0x3c, &(PE_OFFSET as u32).to_le_bytes());
    put(image, PE_OFFSET, b"PE\0\0");

    let mut dll_chars = IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE;
    if h.nxcompat {
        dll_chars |= IMAGE_DLLCHARACTERISTICS_NX_COMPAT;
    }
    if h.relocatable {
        dll_chars |=
            IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE | IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA;
    }

    // COFF file header.
    put(image, coff, &coff::MACHINE_AMD64.to_le_bytes());
    put(image, coff + 2, &(sections.len() as u16).to_le_bytes());
    put(image, coff + 16, &(OPTIONAL_HEADER_SIZE as u16).to_le_bytes());
    let file_chars = IMAGE_FILE_EXECUTABLE_IMAGE | IMAGE_FILE_LARGE_ADDRESS_AWARE;
    put(image, coff + 18, &file_chars.to_le_bytes());

    // Optional header, PE32+ flavour.
    put(image, opt, &0x20bu16.to_le_bytes());
    put(image, opt + 4, &(h.code_size as u32).to_le_bytes());
    put(image, opt + 8, &(h.init_size as u32).to_le_bytes());
    put(image, opt + 12, &(h.uninit_size as u32).to_le_bytes());
    put(image, opt + 16, &(h.entry_rva as u32).to_le_bytes());
    put(image, opt + 20, &(h.base_of_code as u32).to_le_bytes());
    put(image, opt + 24, &h.image_base.to_le_bytes());
    put(image, opt + 32, &(SECTION_ALIGN as u32).to_le_bytes());
    put(image, opt + 36, &(FILE_ALIGN as u32).to_le_bytes());
    put(image, opt + 40, &6u16.to_le_bytes());
    put(image, opt + 48, &6u16.to_le_bytes());
    put(image, opt + 56, &(h.size_of_image as u32).to_le_bytes());
    put(image, opt + 60, &(h.size_of_headers as u32).to_le_bytes());
    put(image, opt + 68, &h.subsystem.to_le_bytes());
    put(image, opt + 70, &dll_chars.to_le_bytes());
    put(image, opt + 72, &0x10_0000u64.to_le_bytes());
    put(image, opt + 80, &0x1000u64.to_le_bytes());
    put(image, opt + 88, &0x10_0000u64.to_le_bytes());
    put(image, opt + 96, &0x1000u64.to_le_bytes());
    put(image, opt + 108, &(NUM_DATA_DIRECTORIES as u32).to_le_bytes());
    let dir = opt + 112 + BASE_RELOC_DIRECTORY * 8;
    put(image, dir, &(h.reloc_dir.0 as u32).to_le_bytes());
    put(image, dir + 4, &(h.reloc_dir.1 as u32).to_le_bytes());

    // Section table. A section with initialized data has no uninitialized
    // flag: its uninitialized tail is just part of its virtual size.
    for (i, o) in sections.iter().enumerate() {
        let at = table + i * SECTION_HEADER_SIZE;
        let n = o.name.len().min(8);
        put(image, at, &[0u8; 8]);
        put(image, at, &o.name[..n]);
        put(image, at + 8, &(o.virt_size as u32).to_le_bytes());
        put(image, at + 12, &(o.rva as u32).to_le_bytes());
        put(image, at + 16, &(o.raw_size as u32).to_le_bytes());
        put(image, at + 20, &(o.raw_off as u32).to_le_bytes());
        let chars = if o.chars & SCN_CNT_INITIALIZED_DATA != 0 {
            o.chars & !SCN_CNT_UNINITIALIZED_DATA
        } else {
            o.chars
        };
        put(image, at + 36, &chars.to_le_bytes());
    }
}
