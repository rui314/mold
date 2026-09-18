//! Byte order markers, integer I/O, and byte-backed integer types.

use std::fmt;
use std::marker::PhantomData;
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

/// Byte order as a type-level marker.
pub trait Endian: Copy + Default + Eq + Send + Sync + fmt::Debug + 'static {
    const IS_LITTLE: bool;

    fn read_u16(bytes: &[u8]) -> u16 {
        if Self::IS_LITTLE {
            read_ul16(bytes)
        } else {
            read_ub16(bytes)
        }
    }

    fn read_u32(bytes: &[u8]) -> u32 {
        if Self::IS_LITTLE {
            read_ul32(bytes)
        } else {
            read_ub32(bytes)
        }
    }

    fn read_u64(bytes: &[u8]) -> u64 {
        if Self::IS_LITTLE {
            read_ul64(bytes)
        } else {
            read_ub64(bytes)
        }
    }

    fn read_i32(bytes: &[u8]) -> i32 {
        if Self::IS_LITTLE {
            read_il32(bytes)
        } else {
            read_ib32(bytes)
        }
    }

    fn read_i64(bytes: &[u8]) -> i64 {
        if Self::IS_LITTLE {
            read_il64(bytes)
        } else {
            read_ib64(bytes)
        }
    }

    fn write_u16(bytes: &mut [u8], value: u16) {
        if Self::IS_LITTLE {
            write_ul16(bytes, value);
        } else {
            write_ub16(bytes, value);
        }
    }

    fn write_u32(bytes: &mut [u8], value: u32) {
        if Self::IS_LITTLE {
            write_ul32(bytes, value);
        } else {
            write_ub32(bytes, value);
        }
    }

    fn write_u64(bytes: &mut [u8], value: u64) {
        if Self::IS_LITTLE {
            write_ul64(bytes, value);
        } else {
            write_ub64(bytes, value);
        }
    }

    fn write_i32(bytes: &mut [u8], value: i32) {
        if Self::IS_LITTLE {
            write_il32(bytes, value);
        } else {
            write_ib32(bytes, value);
        }
    }

    fn write_i64(bytes: &mut [u8], value: i64) {
        if Self::IS_LITTLE {
            write_il64(bytes, value);
        } else {
            write_ib64(bytes, value);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LittleEndian;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BigEndian;

impl Endian for LittleEndian {
    const IS_LITTLE: bool = true;
}

impl Endian for BigEndian {
    const IS_LITTLE: bool = false;
}

macro_rules! endian_integer {
    ($name:ident, $int:ty, $size:expr, $read:ident, $write:ident) => {
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name<E: Endian> {
            bytes: [u8; $size],
            endian: PhantomData<E>,
        }

        impl<E: Endian> $name<E> {
            #[inline(always)]
            pub fn new(value: $int) -> Self {
                let mut result = Self::default();
                result.set(value);
                result
            }

            #[inline(always)]
            pub fn get(&self) -> $int {
                E::$read(&self.bytes)
            }

            #[inline(always)]
            pub fn set(&mut self, value: $int) {
                E::$write(&mut self.bytes, value);
            }
        }
    };
}

endian_integer!(U16, u16, 2, read_u16, write_u16);
endian_integer!(U32, u32, 4, read_u32, write_u32);
endian_integer!(U64, u64, 8, read_u64, write_u64);
endian_integer!(I32, i32, 4, read_i32, write_i32);
endian_integer!(I64, i64, 8, read_i64, write_i64);

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct U24<E: Endian> {
    bytes: [u8; 3],
    endian: PhantomData<E>,
}

impl<E: Endian> U24<E> {
    #[inline(always)]
    pub fn get(&self) -> u32 {
        if E::IS_LITTLE {
            u32::from_le_bytes([self.bytes[0], self.bytes[1], self.bytes[2], 0])
        } else {
            u32::from_be_bytes([0, self.bytes[0], self.bytes[1], self.bytes[2]])
        }
    }

    #[inline(always)]
    pub fn set(&mut self, value: u32) {
        let bytes = if E::IS_LITTLE { value.to_le_bytes() } else { value.to_be_bytes() };
        if E::IS_LITTLE {
            self.bytes.copy_from_slice(&bytes[..3]);
        } else {
            self.bytes.copy_from_slice(&bytes[1..]);
        }
    }
}

pub(crate) type Ul24 = U24<LittleEndian>;
pub(crate) type Ul32 = U32<LittleEndian>;
pub(crate) type Ul64 = U64<LittleEndian>;
pub(crate) type Il32 = I32<LittleEndian>;
pub(crate) type Il64 = I64<LittleEndian>;

pub(crate) type Ub24 = U24<BigEndian>;
pub(crate) type Ub32 = U32<BigEndian>;
pub(crate) type Ub64 = U64<BigEndian>;
pub(crate) type Ib32 = I32<BigEndian>;
pub(crate) type Ib64 = I64<BigEndian>;
