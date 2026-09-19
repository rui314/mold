//! The little DWARF reading the linker needs: the name and compilation
//! directory of an object's compile unit, for the N_SO debug-note stabs
//! ld64 writes ahead of the N_OSO that points debuggers at the object.

use crate::macho::format::MachSection;

const DW_TAG_COMPILE_UNIT: u64 = 0x11;
const DW_AT_NAME: u64 = 0x03;
const DW_AT_COMP_DIR: u64 = 0x1b;
const DW_AT_STR_OFFSETS_BASE: u64 = 0x72;

const DW_FORM_ADDR: u64 = 0x01;
const DW_FORM_BLOCK2: u64 = 0x03;
const DW_FORM_BLOCK4: u64 = 0x04;
const DW_FORM_DATA2: u64 = 0x05;
const DW_FORM_DATA4: u64 = 0x06;
const DW_FORM_DATA8: u64 = 0x07;
const DW_FORM_STRING: u64 = 0x08;
const DW_FORM_BLOCK: u64 = 0x09;
const DW_FORM_BLOCK1: u64 = 0x0a;
const DW_FORM_DATA1: u64 = 0x0b;
const DW_FORM_FLAG: u64 = 0x0c;
const DW_FORM_SDATA: u64 = 0x0d;
const DW_FORM_STRP: u64 = 0x0e;
const DW_FORM_UDATA: u64 = 0x0f;
const DW_FORM_REF_ADDR: u64 = 0x10;
const DW_FORM_REF1: u64 = 0x11;
const DW_FORM_REF2: u64 = 0x12;
const DW_FORM_REF4: u64 = 0x13;
const DW_FORM_REF8: u64 = 0x14;
const DW_FORM_REF_UDATA: u64 = 0x15;
const DW_FORM_INDIRECT: u64 = 0x16;
const DW_FORM_SEC_OFFSET: u64 = 0x17;
const DW_FORM_EXPRLOC: u64 = 0x18;
const DW_FORM_FLAG_PRESENT: u64 = 0x19;
const DW_FORM_STRX: u64 = 0x1a;
const DW_FORM_ADDRX: u64 = 0x1b;
const DW_FORM_REF_SUP4: u64 = 0x1c;
const DW_FORM_STRP_SUP: u64 = 0x1d;
const DW_FORM_DATA16: u64 = 0x1e;
const DW_FORM_LINE_STRP: u64 = 0x1f;
const DW_FORM_REF_SIG8: u64 = 0x20;
const DW_FORM_IMPLICIT_CONST: u64 = 0x21;
const DW_FORM_LOCLISTX: u64 = 0x22;
const DW_FORM_RNGLISTX: u64 = 0x23;
const DW_FORM_REF_SUP8: u64 = 0x24;
const DW_FORM_STRX1: u64 = 0x25;
const DW_FORM_STRX2: u64 = 0x26;
const DW_FORM_STRX3: u64 = 0x27;
const DW_FORM_STRX4: u64 = 0x28;
const DW_FORM_ADDRX1: u64 = 0x29;
const DW_FORM_ADDRX2: u64 = 0x2a;
const DW_FORM_ADDRX3: u64 = 0x2b;
const DW_FORM_ADDRX4: u64 = 0x2c;

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let b = self.data.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_le_bytes(b.try_into().unwrap()))
    }
    fn u32(&mut self) -> Option<u32> {
        let b = self.data.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn u64(&mut self) -> Option<u64> {
        let b = self.data.get(self.pos..self.pos + 8)?;
        self.pos += 8;
        Some(u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn uleb(&mut self) -> Option<u64> {
        let mut val = 0u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            if shift < 64 {
                val |= ((b & 0x7f) as u64) << shift;
            }
            shift += 7;
            if b & 0x80 == 0 {
                return Some(val);
            }
        }
    }
    fn sleb(&mut self) -> Option<i64> {
        let mut val = 0i64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            if shift < 64 {
                val |= ((b & 0x7f) as i64) << shift;
            }
            shift += 7;
            if b & 0x80 == 0 {
                if shift < 64 && b & 0x40 != 0 {
                    val |= -1i64 << shift;
                }
                return Some(val);
            }
        }
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        if self.pos + n > self.data.len() {
            return None;
        }
        self.pos += n;
        Some(())
    }
    fn cstr(&mut self) -> Option<&'a [u8]> {
        let rest = self.data.get(self.pos..)?;
        let n = rest.iter().position(|&b| b == 0)?;
        self.pos += n + 1;
        Some(&rest[..n])
    }
}

fn cstr_at(data: &[u8], off: usize) -> Option<&[u8]> {
    let rest = data.get(off..)?;
    let n = rest.iter().position(|&b| b == 0)?;
    Some(&rest[..n])
}

/// What an attribute's value came as: a string, a string-section
/// offset, or a string-offsets-table index.
enum Value<'a> {
    Inline(&'a [u8]),
    Strp(u64),
    LineStrp(u64),
    Strx(u64),
    Other(u64),
}

/// Reads one attribute value of the given form, returning it when it
/// can be a string.
fn read_form<'a>(
    r: &mut Reader<'a>,
    form: u64,
    addr_size: usize,
    implicit: i64,
) -> Option<Value<'a>> {
    Some(match form {
        DW_FORM_ADDR => Value::Other(match addr_size {
            4 => r.u32()? as u64,
            _ => r.u64()?,
        }),
        DW_FORM_BLOCK2 => {
            let n = r.u16()? as usize;
            r.skip(n)?;
            Value::Other(0)
        }
        DW_FORM_BLOCK4 => {
            let n = r.u32()? as usize;
            r.skip(n)?;
            Value::Other(0)
        }
        DW_FORM_DATA2 | DW_FORM_REF2 => Value::Other(r.u16()? as u64),
        DW_FORM_DATA4 | DW_FORM_REF4 | DW_FORM_REF_ADDR | DW_FORM_SEC_OFFSET | DW_FORM_REF_SUP4
        | DW_FORM_STRP_SUP => Value::Other(r.u32()? as u64),
        DW_FORM_DATA8 | DW_FORM_REF8 | DW_FORM_REF_SIG8 | DW_FORM_REF_SUP8 => {
            Value::Other(r.u64()?)
        }
        DW_FORM_STRING => Value::Inline(r.cstr()?),
        DW_FORM_BLOCK | DW_FORM_EXPRLOC => {
            let n = r.uleb()? as usize;
            r.skip(n)?;
            Value::Other(0)
        }
        DW_FORM_BLOCK1 => {
            let n = r.u8()? as usize;
            r.skip(n)?;
            Value::Other(0)
        }
        DW_FORM_DATA1 | DW_FORM_REF1 | DW_FORM_FLAG => Value::Other(r.u8()? as u64),
        DW_FORM_SDATA => Value::Other(r.sleb()? as u64),
        DW_FORM_STRP => Value::Strp(r.u32()? as u64),
        DW_FORM_LINE_STRP => Value::LineStrp(r.u32()? as u64),
        DW_FORM_UDATA | DW_FORM_REF_UDATA | DW_FORM_ADDRX | DW_FORM_LOCLISTX | DW_FORM_RNGLISTX => {
            Value::Other(r.uleb()?)
        }
        DW_FORM_INDIRECT => {
            let actual = r.uleb()?;
            return read_form(r, actual, addr_size, implicit);
        }
        DW_FORM_FLAG_PRESENT => Value::Other(1),
        DW_FORM_STRX => Value::Strx(r.uleb()?),
        DW_FORM_STRX1 => Value::Strx(r.u8()? as u64),
        DW_FORM_STRX2 => Value::Strx(r.u16()? as u64),
        DW_FORM_STRX3 => {
            let b = [r.u8()?, r.u8()?, r.u8()?, 0];
            Value::Strx(u32::from_le_bytes(b) as u64)
        }
        DW_FORM_STRX4 => Value::Strx(r.u32()? as u64),
        DW_FORM_ADDRX1 => Value::Other(r.u8()? as u64),
        DW_FORM_ADDRX2 => Value::Other(r.u16()? as u64),
        DW_FORM_ADDRX3 => {
            r.skip(3)?;
            Value::Other(0)
        }
        DW_FORM_ADDRX4 => Value::Other(r.u32()? as u64),
        DW_FORM_DATA16 => {
            r.skip(16)?;
            Value::Other(0)
        }
        DW_FORM_IMPLICIT_CONST => Value::Other(implicit as u64),
        _ => return None,
    })
}

/// The compilation directory and source file name of an object's first
/// compile unit (DW_AT_comp_dir and DW_AT_name of the DW_TAG_compile_unit
/// DIE), read from its __DWARF sections. DWARF versions 2 through 5;
/// strings inline, in __debug_str or __debug_line_str, or indexed
/// through __debug_str_offsets.
pub fn compile_unit_name(file: &[u8], sects: &[MachSection]) -> Option<(String, String)> {
    let section = |name: &str| -> Option<&[u8]> {
        let s = sects.iter().find(|s| s.segname() == "__DWARF" && s.sectname() == name)?;
        file.get(s.offset as usize..(s.offset as u64 + s.size) as usize)
    };
    let info = section("__debug_info")?;
    let abbrev_sect = section("__debug_abbrev")?;
    let str_sect = section("__debug_str").unwrap_or(&[]);
    let line_str_sect = section("__debug_line_str").unwrap_or(&[]);
    // Mach-O section names are 16 bytes: "__debug_str_offsets" is truncated.
    let str_offsets_sect = section("__debug_str_offs").unwrap_or(&[]);

    let mut r = Reader { data: info, pos: 0 };
    let length = r.u32()?;
    if length == 0xffff_ffff {
        return None; // 64-bit DWARF: not produced by Apple's compilers
    }
    let version = r.u16()?;
    let (abbrev_off, addr_size) = if version >= 5 {
        let _unit_type = r.u8()?;
        let addr_size = r.u8()?;
        (r.u32()? as usize, addr_size as usize)
    } else {
        let abbrev_off = r.u32()? as usize;
        (abbrev_off, r.u8()? as usize)
    };

    // The abbreviation of the first DIE.
    let code = r.uleb()?;
    let mut a = Reader { data: abbrev_sect.get(abbrev_off..)?, pos: 0 };
    let specs: Vec<(u64, u64, i64)> = loop {
        let c = a.uleb()?;
        if c == 0 {
            return None;
        }
        let tag = a.uleb()?;
        let _children = a.u8()?;
        let mut specs = Vec::new();
        loop {
            let attr = a.uleb()?;
            let form = a.uleb()?;
            if attr == 0 && form == 0 {
                break;
            }
            let implicit = if form == DW_FORM_IMPLICIT_CONST { a.sleb()? } else { 0 };
            specs.push((attr, form, implicit));
        }
        if c == code {
            if tag != DW_TAG_COMPILE_UNIT {
                return None;
            }
            break specs;
        }
    };

    let mut name = None;
    let mut comp_dir = None;
    let mut str_offsets_base = None;
    for &(attr, form, implicit) in &specs {
        let val = read_form(&mut r, form, addr_size, implicit)?;
        match attr {
            DW_AT_NAME => name = Some(val),
            DW_AT_COMP_DIR => comp_dir = Some(val),
            DW_AT_STR_OFFSETS_BASE => {
                if let Value::Other(v) = val {
                    str_offsets_base = Some(v as usize);
                }
            }
            _ => {}
        }
    }

    let resolve = |v: Value| -> Option<String> {
        let bytes = match v {
            Value::Inline(b) => b,
            Value::Strp(off) => cstr_at(str_sect, off as usize)?,
            Value::LineStrp(off) => cstr_at(line_str_sect, off as usize)?,
            Value::Strx(idx) => {
                // The table's header is 8 bytes; a unit's base normally
                // points just past it.
                let base = str_offsets_base.unwrap_or(8);
                let at = base + idx as usize * 4;
                let off = u32::from_le_bytes(str_offsets_sect.get(at..at + 4)?.try_into().unwrap());
                cstr_at(str_sect, off as usize)?
            }
            Value::Other(_) => return None,
        };
        Some(String::from_utf8_lossy(bytes).into_owned())
    };
    let name = name.and_then(resolve)?;
    let comp_dir = comp_dir.and_then(resolve).unwrap_or_default();
    Some((comp_dir, name))
}
