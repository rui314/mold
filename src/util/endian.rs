//! Fixed-byte-order integer I/O on byte slices.
//!
//! These functions read and write integers of a known byte order, for code
//! that patches section contents of a known target. Layout-generic code
//! uses the methods on [`crate::elf::Layout`], which pick the byte order
//! from the target.

use std::mem::size_of;

macro_rules! endian_io {
    ($int:ty, $readb:ident, $readl:ident, $writeb:ident, $writel:ident) => {
        #[inline]
        pub fn $readb(bytes: &[u8]) -> $int {
            let bytes = bytes[..size_of::<$int>()].try_into().unwrap();
            <$int>::from_be_bytes(bytes)
        }

        #[inline]
        pub fn $readl(bytes: &[u8]) -> $int {
            let bytes = bytes[..size_of::<$int>()].try_into().unwrap();
            <$int>::from_le_bytes(bytes)
        }

        #[inline]
        pub fn $writeb(bytes: &mut [u8], value: $int) {
            bytes[..size_of::<$int>()].copy_from_slice(&value.to_be_bytes());
        }

        #[inline]
        pub fn $writel(bytes: &mut [u8], value: $int) {
            bytes[..size_of::<$int>()].copy_from_slice(&value.to_le_bytes());
        }
    };
}

endian_io!(u16, read_ub16, read_ul16, write_ub16, write_ul16);
endian_io!(u32, read_ub32, read_ul32, write_ub32, write_ul32);
endian_io!(u64, read_ub64, read_ul64, write_ub64, write_ul64);
endian_io!(i16, read_ib16, read_il16, write_ib16, write_il16);
endian_io!(i32, read_ib32, read_il32, write_ib32, write_il32);
endian_io!(i64, read_ib64, read_il64, write_ib64, write_il64);
