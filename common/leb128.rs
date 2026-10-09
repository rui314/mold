//! LEB128, the variable-length integer encoding of DWARF and of other
//! tables in object files.

use crate::bits::sign_extend;

/// Appends `value` in unsigned LEB128 encoding.
pub fn encode_uleb(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Appends `value` in signed LEB128 encoding.
pub fn encode_sleb(out: &mut Vec<u8>, mut value: i64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        let negative = byte & 0x40 != 0;
        if (value == 0 && !negative) || (value == -1 && negative) {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Writes `value` in unsigned LEB128 encoding at the start of `buf`,
/// returning its length.
pub fn write_uleb(buf: &mut [u8], mut value: u64) -> usize {
    let mut i = 0;
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf[i] = byte;
        i += 1;
        if value == 0 {
            return i;
        }
    }
}

/// The length of `value` in unsigned LEB128 encoding.
pub fn uleb_size(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

/// Overwrites an existing unsigned LEB128 value in place, keeping its length.
pub fn overwrite_uleb(buf: &mut [u8], mut value: u64) {
    let mut i = 0;
    while buf[i] & 0x80 != 0 {
        buf[i] = 0x80 | (value & 0x7f) as u8;
        value >>= 7;
        i += 1;
    }
    buf[i] = (value & 0x7f) as u8;
}

/// Reads an unsigned LEB128 value, advancing `bytes` past it.
#[inline]
pub fn read_uleb(bytes: &mut &[u8]) -> u64 {
    let mut value = 0;
    let mut shift = 0;
    loop {
        let (&byte, rest) = bytes.split_first().expect("truncated LEB128");
        *bytes = rest;
        if shift < 64 {
            value |= ((byte & 0x7f) as u64) << shift;
        }
        shift += 7;
        if byte & 0x80 == 0 {
            return value;
        }
    }
}

/// Reads a signed LEB128 value, advancing `bytes` past it.
#[inline]
pub fn read_sleb(bytes: &mut &[u8]) -> i64 {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let (&byte, rest) = bytes.split_first().expect("truncated LEB128");
        *bytes = rest;
        if shift < 64 {
            value |= ((byte & 0x7f) as u64) << shift;
        }
        shift += 7;
        if byte & 0x80 == 0 {
            return if shift < 64 { sign_extend(value, shift) } else { value as i64 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb128_roundtrip() {
        for &value in &[0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut buf = Vec::new();
            encode_uleb(&mut buf, value);
            assert_eq!(uleb_size(value), buf.len());
            let mut written = [0xff; 10];
            assert_eq!(write_uleb(&mut written, value), buf.len());
            assert_eq!(&written[..buf.len()], buf);
            let mut slice = buf.as_slice();
            assert_eq!(read_uleb(&mut slice), value);
            assert!(slice.is_empty());
        }
        for &value in &[0i64, -1, 63, 64, -64, -65, i64::MIN, i64::MAX] {
            let mut buf = Vec::new();
            encode_sleb(&mut buf, value);
            let mut slice = buf.as_slice();
            assert_eq!(read_sleb(&mut slice), value);
        }
    }
}
