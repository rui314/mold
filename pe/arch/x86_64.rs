//! The x86_64 target: its machine type and COFF relocations, as the Microsoft
//! PE/COFF specification and lld define them.

use super::{Arch, BASE_DIR64, BASE_HIGHLOW, Fixup, RelocError, Target};

/// `IMAGE_FILE_MACHINE_AMD64`.
pub const MACHINE: u16 = 0x8664;

const IMAGE_REL_AMD64_ABSOLUTE: u16 = 0;
const IMAGE_REL_AMD64_ADDR64: u16 = 1;
const IMAGE_REL_AMD64_ADDR32: u16 = 2;
const IMAGE_REL_AMD64_ADDR32NB: u16 = 3;
const IMAGE_REL_AMD64_REL32: u16 = 4;
const IMAGE_REL_AMD64_REL32_5: u16 = 9;

/// The x86_64 architecture.
pub struct X86_64;

impl Arch for X86_64 {
    const MACHINE: u16 = MACHINE;

    fn has_base_reloc(kind: u16) -> bool {
        kind == IMAGE_REL_AMD64_ADDR64 || kind == IMAGE_REL_AMD64_ADDR32
    }

    fn apply(image: &mut [u8], fixup: Fixup) -> Result<Option<u8>, RelocError> {
        let Fixup { kind, at, rva, target, image_base } = fixup;
        let va = match target {
            Target::Image(rva) => image_base + rva,
            Target::Abs(v) => v,
        };
        let in_image = matches!(target, Target::Image(_));
        match kind {
            IMAGE_REL_AMD64_ABSOLUTE => Ok(None),
            IMAGE_REL_AMD64_ADDR64 => {
                // COFF relocations add to the field, which holds the addend.
                let addend = u64::from_le_bytes(image[at..at + 8].try_into().unwrap());
                image[at..at + 8].copy_from_slice(&addend.wrapping_add(va).to_le_bytes());
                Ok(in_image.then_some(BASE_DIR64))
            }
            IMAGE_REL_AMD64_ADDR32 => {
                let addend = u32::from_le_bytes(image[at..at + 4].try_into().unwrap());
                image[at..at + 4].copy_from_slice(&addend.wrapping_add(va as u32).to_le_bytes());
                Ok(in_image.then_some(BASE_HIGHLOW))
            }
            IMAGE_REL_AMD64_ADDR32NB => {
                let value = match target {
                    Target::Image(rva) => rva,
                    Target::Abs(v) => v,
                };
                let addend = u32::from_le_bytes(image[at..at + 4].try_into().unwrap());
                image[at..at + 4].copy_from_slice(&addend.wrapping_add(value as u32).to_le_bytes());
                Ok(None)
            }
            IMAGE_REL_AMD64_REL32..=IMAGE_REL_AMD64_REL32_5 => {
                // IMAGE_REL_AMD64_REL32_n add n for the bytes that follow the field.
                let extra = (kind - IMAGE_REL_AMD64_REL32) as u64;
                let pc = (image_base + rva + 4 + extra) as i64;
                let addend = i32::from_le_bytes(image[at..at + 4].try_into().unwrap()) as i64;
                let delta =
                    i32::try_from(addend + va as i64 - pc).map_err(|_| RelocError::OutOfRange)?;
                image[at..at + 4].copy_from_slice(&delta.to_le_bytes());
                Ok(None)
            }
            _ => Err(RelocError::Unsupported(kind)),
        }
    }
}
