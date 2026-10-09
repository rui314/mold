//! Reads COFF object files, the format of the `.obj` and `.o` files and of
//! the archive members that rustc and clang emit for x86_64 Windows and UEFI
//! targets.

use crate::arch::x86_64::MACHINE;

pub const SCN_CNT_CODE: u32 = 0x0000_0020;
pub const SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
pub const SCN_CNT_UNINITIALIZED_DATA: u32 = 0x0000_0080;
pub const SCN_LNK_INFO: u32 = 0x0000_0200;
pub const SCN_LNK_REMOVE: u32 = 0x0000_0800;
pub const SCN_LNK_NRELOC_OVFL: u32 = 0x0100_0000;
pub const SCN_MEM_EXECUTE: u32 = 0x2000_0000;
pub const SCN_MEM_READ: u32 = 0x4000_0000;
pub const SCN_MEM_WRITE: u32 = 0x8000_0000;
pub const SCN_ALIGN_MASK: u32 = 0x00f0_0000;

pub const SYM_ABSOLUTE: i16 = -1;
pub const SYM_DEBUG: i16 = -2;

pub const CLASS_EXTERNAL: u8 = 2;
pub const CLASS_STATIC: u8 = 3;
pub const CLASS_FILE: u8 = 103;
pub const CLASS_SECTION: u8 = 104;
pub const CLASS_WEAK_EXTERNAL: u8 = 105;

/// COMDAT selection that keeps a section only if its associated section is kept.
pub const SEL_ASSOCIATIVE: u8 = 5;

const SYMBOL_SIZE: usize = 18;
const SECTION_HEADER_SIZE: usize = 40;
const RELOC_SIZE: usize = 10;

/// A parsed object file. Symbol indices are COFF symbol table indices, so
/// auxiliary records keep their slots in `symbols`, marked by `aux_slot`.
pub struct Object<'a> {
    pub name: String,
    pub sections: Vec<Section<'a>>,
    pub symbols: Vec<Symbol<'a>>,
}

pub struct Section<'a> {
    pub name: &'a [u8],
    pub characteristics: u32,
    /// The raw contents. Empty for uninitialized sections.
    pub data: &'a [u8],
    /// The size in bytes, which for uninitialized sections is the size in memory.
    pub size: u32,
    /// Set if the section is a COMDAT section.
    pub comdat: Option<Comdat>,
    pub relocs: Vec<Reloc>,
}

#[derive(Clone, Copy)]
pub struct Comdat {
    pub selection: u8,
    /// The 1-based number of the section that an associative COMDAT belongs to.
    pub associated: u16,
}

#[derive(Clone, Copy)]
pub struct Reloc {
    /// Offset of the relocated field from the start of its section.
    pub offset: u32,
    pub symbol: u32,
    pub kind: u16,
}

pub struct Symbol<'a> {
    pub name: &'a [u8],
    pub value: u32,
    /// 1-based section number, or one of the special values 0, -1 and -2.
    pub section: i16,
    pub storage: u8,
    /// True for the auxiliary records that follow a symbol.
    pub aux_slot: bool,
    /// For a weak external, the index of the symbol it defaults to.
    pub weak_default: Option<u32>,
}

/// Returns true if `data` starts with a COFF file header for x86_64.
pub fn is_coff_object(data: &[u8]) -> bool {
    data.len() >= 2 && u16::from_le_bytes([data[0], data[1]]) == MACHINE
}

pub fn parse<'a>(name: String, data: &'a [u8]) -> Result<Object<'a>, String> {
    let truncated = || format!("{name}: truncated COFF object file");
    let header = get(data, 0, 20).ok_or_else(truncated)?;
    let machine = le16(&header[0..2]);
    if machine != MACHINE {
        return Err(format!("{name}: unsupported machine type 0x{machine:04x}"));
    }
    let nsec = le16(&header[2..4]) as usize;
    let symtab = le32(&header[8..12]) as usize;
    let nsyms = le32(&header[12..16]) as usize;
    let opt_size = le16(&header[16..18]) as usize;

    let strings = string_table(data, symtab, nsyms).ok_or_else(truncated)?;

    let shdrs = get(data, 20 + opt_size, nsec * SECTION_HEADER_SIZE).ok_or_else(truncated)?;
    let mut sections = Vec::with_capacity(nsec);
    for h in shdrs.chunks_exact(SECTION_HEADER_SIZE) {
        let name = if h[0] == b'/' {
            let digits: Vec<u8> = h[1..8].iter().copied().take_while(u8::is_ascii_digit).collect();
            let off = std::str::from_utf8(&digits).ok().and_then(|s| s.parse().ok());
            str_at(strings, off.unwrap_or(0))
        } else {
            trim_nul(&h[0..8])
        };
        let characteristics = le32(&h[36..40]);
        let size = le32(&h[16..20]);
        let raw_ptr = le32(&h[20..24]) as usize;
        let data_bytes = if characteristics & SCN_CNT_UNINITIALIZED_DATA != 0 || size == 0 {
            &data[..0]
        } else {
            get(data, raw_ptr, size as usize).ok_or_else(truncated)?
        };
        let relocs = read_relocs(data, &h[24..28], &h[32..34], characteristics, name)?;
        sections.push(Section {
            name,
            characteristics,
            data: data_bytes,
            size,
            comdat: None,
            relocs,
        });
    }

    let symtab_bytes = get(data, symtab, nsyms * SYMBOL_SIZE).ok_or_else(truncated)?;
    let mut symbols: Vec<Symbol<'a>> = Vec::with_capacity(nsyms);
    let mut i = 0;
    while i < nsyms {
        let r = &symtab_bytes[i * SYMBOL_SIZE..(i + 1) * SYMBOL_SIZE];
        let sym_name = if le32(&r[0..4]) == 0 {
            str_at(strings, le32(&r[4..8]) as usize)
        } else {
            trim_nul(&r[0..8])
        };
        let section = le16(&r[12..14]) as i16;
        let storage = r[16];
        let naux = r[17] as usize;
        if i + naux >= nsyms {
            return Err(truncated());
        }
        let index = symbols.len();
        symbols.push(Symbol {
            name: sym_name,
            value: le32(&r[8..12]),
            section,
            storage,
            aux_slot: false,
            weak_default: None,
        });
        for j in 0..naux {
            let a = &symtab_bytes[(i + 1 + j) * SYMBOL_SIZE..(i + 2 + j) * SYMBOL_SIZE];
            if j == 0 && storage == CLASS_STATIC && section > 0 {
                // A section definition: its auxiliary record says how the section is COMDAT.
                let sec = sections
                    .get_mut(section as usize - 1)
                    .ok_or_else(|| format!("{name}: invalid section number {section}"))?;
                let selection = a[14];
                if selection != 0 {
                    sec.comdat = Some(Comdat { selection, associated: le16(&a[12..14]) });
                }
            }
            if j == 0 && storage == CLASS_WEAK_EXTERNAL {
                symbols[index].weak_default = Some(le32(&a[0..4]));
            }
            symbols.push(Symbol {
                name: &[],
                value: 0,
                section: 0,
                storage: 0,
                aux_slot: true,
                weak_default: None,
            });
        }
        i += 1 + naux;
    }

    for sec in &sections {
        for r in &sec.relocs {
            if r.symbol as usize >= symbols.len() {
                return Err(format!(
                    "{name}: relocation refers to symbol {} out of range",
                    r.symbol
                ));
            }
        }
    }

    Ok(Object { name, sections, symbols })
}

fn read_relocs(
    data: &[u8],
    ptr: &[u8],
    count: &[u8],
    characteristics: u32,
    sec_name: &[u8],
) -> Result<Vec<Reloc>, String> {
    let ptr = le32(ptr) as usize;
    let mut count = le16(count) as usize;
    let mut first = 0;
    if characteristics & SCN_LNK_NRELOC_OVFL != 0 && count == 0xffff {
        // The real count, including this record, is in the first record's address.
        let rec =
            get(data, ptr, RELOC_SIZE).ok_or_else(|| "truncated relocation table".to_string())?;
        count = (le32(&rec[0..4]) as usize).saturating_sub(1);
        first = 1;
    }
    let bytes = get(data, ptr + first * RELOC_SIZE, count * RELOC_SIZE).ok_or_else(|| {
        format!("truncated relocation table in section {}", String::from_utf8_lossy(sec_name))
    })?;
    Ok(bytes
        .chunks_exact(RELOC_SIZE)
        .map(|r| Reloc { offset: le32(&r[0..4]), symbol: le32(&r[4..8]), kind: le16(&r[8..10]) })
        .collect())
}

/// Returns the string table, which follows the symbol table, or an empty
/// slice if there is none. Name offsets are relative to its start, which
/// includes its own 4-byte size field.
fn string_table(data: &[u8], symtab: usize, nsyms: usize) -> Option<&[u8]> {
    if symtab == 0 {
        return Some(&[]);
    }
    let off = symtab.checked_add(nsyms.checked_mul(SYMBOL_SIZE)?)?;
    match get(data, off, 4) {
        Some(len) => get(data, off, le32(len) as usize),
        None => Some(&[]),
    }
}

fn str_at(strings: &[u8], off: usize) -> &[u8] {
    let s = strings.get(off..).unwrap_or(&[]);
    &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
}

fn trim_nul(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

fn get(data: &[u8], off: usize, len: usize) -> Option<&[u8]> {
    data.get(off..off.checked_add(len)?)
}

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
