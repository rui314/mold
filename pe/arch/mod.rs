//! The parts of the PE linker that depend on the processor architecture. The
//! rest of the linker reads, lays out and writes images in the same way for
//! every machine. An architecture supplies its COFF machine type and applies
//! the relocations that its code uses.

pub mod x86_64;

/// The base relocation type for a 32-bit absolute address (`IMAGE_REL_BASED_HIGHLOW`).
pub(crate) const BASE_HIGHLOW: u8 = 3;

/// The base relocation type for a 64-bit absolute address (`IMAGE_REL_BASED_DIR64`).
pub(crate) const BASE_DIR64: u8 = 10;

/// Where a relocation points.
#[derive(Clone, Copy)]
pub(crate) enum Target {
    /// An RVA in the image, to which the image base is added.
    Image(u64),
    /// An absolute value, which is used as it is.
    Abs(u64),
}

/// One relocation to apply to the image.
#[derive(Clone, Copy)]
pub(crate) struct Fixup {
    /// The `IMAGE_REL_*` type of the relocation.
    pub kind: u16,
    /// The offset of the relocated field in the image bytes.
    pub at: usize,
    /// The RVA of the relocated field.
    pub rva: u64,
    pub target: Target,
    pub image_base: u64,
}

/// Why a relocation couldn't be applied.
pub(crate) enum RelocError {
    /// The relocation type is not one that this architecture uses.
    Unsupported(u16),
    /// The target is too far away for the relocation's field.
    OutOfRange,
}

/// An architecture that the linker can produce images for.
pub(crate) trait Arch {
    /// The COFF machine type (`IMAGE_FILE_MACHINE_*`) of the architecture.
    const MACHINE: u16;

    /// Returns true if a relocation of this type can leave an absolute
    /// address in the image, which then needs a base relocation entry.
    fn has_base_reloc(kind: u16) -> bool;

    /// Applies a relocation to `image`. Returns the base relocation type
    /// that the relocated field needs, if the field holds an image address.
    fn apply(image: &mut [u8], fixup: Fixup) -> Result<Option<u8>, RelocError>;
}
