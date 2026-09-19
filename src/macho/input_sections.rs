//! Input sections.

use crate::macho::output_chunks::ChunkId;

/// A subsection index, u32 as in mold-rust.
pub type InputSectionId = u32;

/// The subsection arena. Indexable by a u32 id or a usize (through
/// Deref to the Vec), so id vectors can be u32 - half the size - while
/// every arena access stays `isecs[id]`.
#[derive(Debug, Default)]
pub struct InputSections(pub Vec<InputSection>);

impl std::ops::Deref for InputSections {
    type Target = Vec<InputSection>;
    #[inline]
    fn deref(&self) -> &Vec<InputSection> {
        &self.0
    }
}
impl std::ops::DerefMut for InputSections {
    #[inline]
    fn deref_mut(&mut self) -> &mut Vec<InputSection> {
        &mut self.0
    }
}
impl std::ops::Index<u32> for InputSections {
    type Output = InputSection;
    #[inline]
    fn index(&self, id: u32) -> &InputSection {
        &self.0[id as usize]
    }
}
impl std::ops::IndexMut<u32> for InputSections {
    #[inline]
    fn index_mut(&mut self, id: u32) -> &mut InputSection {
        &mut self.0[id as usize]
    }
}
impl std::ops::Index<usize> for InputSections {
    type Output = InputSection;
    #[inline]
    fn index(&self, id: usize) -> &InputSection {
        &self.0[id]
    }
}
impl std::ops::IndexMut<usize> for InputSections {
    #[inline]
    fn index_mut(&mut self, id: usize) -> &mut InputSection {
        &mut self.0[id]
    }
}

/// What a relocation refers to.
#[derive(Clone, Copy, Debug)]
pub enum RelocTarget {
    /// An index into the owning object's symbol list.
    Sym(u32),
    /// A subsection. During target-specific relocation reading this is
    /// an index into the object's section header list; input file
    /// parsing rewrites it to an index into the global subsection
    /// arena.
    Section(u32),
}

/// A relocation in a form independent of the raw Mach-O records: the
/// addend is explicit, and the target is a symbol or a section.
#[derive(Clone, Copy, Debug)]
pub struct Reloc {
    /// Offset within the containing section.
    pub offset: u32,
    pub r_type: u8,
    /// Size in bytes of the relocated field.
    pub size: u8,
    pub is_pcrel: bool,
    /// True if the previous record is a SUBTRACTOR paired with this one.
    pub is_subtracted: bool,
    /// The target, packed into one word: a symbol or subsection index
    /// in the low 31 bits, with `TARGET_SECTION` set for a subsection.
    /// Read through `target()`, write through `set_target()` or
    /// `RelocTarget::pack()`; as a two-word enum it padded the struct
    /// from 24 to 32 bytes, and a debug link holds ~12M of these.
    pub target: u32,
    pub addend: i64,
}

// A Reloc is the size of an ELF RELA entry, which mold-rust reads from
// the mapping without materializing anything; Mach-O needs the record
// processed (ADDEND fusion, SUBTRACTOR pairing, subsection rebasing),
// so this is the form that processing produces.
const _: () = assert!(std::mem::size_of::<Reloc>() == 24);

const TARGET_SECTION: u32 = 1 << 31;

impl RelocTarget {
    #[inline]
    pub fn pack(self) -> u32 {
        match self {
            RelocTarget::Sym(i) => {
                debug_assert!(i & TARGET_SECTION == 0);
                i
            }
            RelocTarget::Section(i) => {
                debug_assert!(i & TARGET_SECTION == 0);
                i | TARGET_SECTION
            }
        }
    }
}

impl Reloc {
    #[inline]
    pub fn target(&self) -> RelocTarget {
        if self.target & TARGET_SECTION != 0 {
            RelocTarget::Section(self.target & !TARGET_SECTION)
        } else {
            RelocTarget::Sym(self.target)
        }
    }
    #[inline]
    pub fn set_target(&mut self, t: RelocTarget) {
        self.target = t.pack();
    }
}

/// A subsection of an input object file's section.
///
/// Mach-O linking granularity is the subsection: objects are built with
/// MH_SUBSECTIONS_VIA_SYMBOLS, and each section is split at its symbols,
/// so that unreferenced pieces can be dead-stripped. `hdr` is the
/// containing section's header; `input_addr` and `size` delimit this
/// piece of it.
/// Sentinel for `InputSection::replacement`: no surviving copy.
pub const NO_REPLACEMENT: u32 = u32::MAX;

#[derive(Debug)]
pub struct InputSection {
    /// Index of the object file this section came from (u32 to keep the
    /// struct small; `usize::MAX` becomes `u32::MAX` for a synthetic
    /// section with no object).
    pub file: u32,
    /// Index of the parent section's header in the owning object's
    /// section list (the internal object's, for a synthesized one):
    /// mold-rust's shndx. Resolved through Context::hdr_of; a u32 index
    /// instead of an 8-byte header pointer. `p2align` is held inline
    /// because it is the one header field the linker raises per
    /// subsection.
    pub shndx: u32,
    pub p2align: u8,
    /// This subsection's address in the object's address space. Object
    /// files stay well under 4 GiB, so a u32 holds it.
    pub input_addr: u32,
    /// The subsection's size. One subsection is far under 4 GiB, so a
    /// u32 holds it; arithmetic with 64-bit addresses casts up.
    pub size: u32,
    /// The subsection contents, as a bare pointer - the length is
    /// `size` - or 0 when there are none (a zero-fill or empty
    /// section). Stored as an integer, not a slice, to save 8 bytes and
    /// keep the struct trivially Send/Sync; read through `data()`.
    /// mold-rust likewise keeps `contents` as a bare address.
    pub contents: usize,
    /// This subsection's relocations: a range in the owning object's
    /// `relocs` arena, offsets relative to the subsection. sold keeps
    /// rel_offset/nrels per subsection the same way, rather than a Vec
    /// per subsection - a debug link has millions of relocations.
    pub rel_offset: u32,
    pub nrels: u32,
    /// The chunk this subsection is laid out in, as `ChunkId::pack`
    /// encodes it, or `u32::MAX` until assigned: read through
    /// `output_section()`. mold-rust's field of this name holds an
    /// Option<OutputSectionId> (a word, thanks to the id's niche); a
    /// Mach-O subsection may also be placed in the GOT (a folded
    /// __objc_classrefs entry) or in __objc_methlist (a rewritten
    /// method list), so the packed ChunkId keeps the struct at 56 bytes.
    pub output_section: u32,
    /// Offset from the start of the output section (u32::MAX marks a
    /// subsection not yet placed, during thunk layout). An output
    /// section stays well under 4 GiB, so a u32 suffices.
    pub offset: u32,
    /// IS_ALIVE and the transient IS_VISITED bit, in one atomic byte so
    /// the parallel dead-strip walk marks sections in place (mold-rust's
    /// InputSection flags). Read through is_alive(); the &mut setters
    /// write without an atomic operation.
    pub flags: std::sync::atomic::AtomicU8,
    /// For a literal merged with an identical one, the surviving copy's
    /// subsection index, or `NO_REPLACEMENT`. A u32 sentinel rather than
    /// an `Option<usize>` (16 bytes) keeps the struct small.
    pub replacement: u32,
    /// This subsection's compact-unwind records: a range in
    /// ctx.unwind_records, as sold keeps unwind_offset/nunwind on each
    /// subsection (records arrive grouped by function).
    pub unwind_offset: u32,
    pub nunwind: u32,
}

// InputSection is the highest-count struct in a link (millions on a
// debug build), so it is kept compact: 56 bytes, under mold-rust's 64
// (ours carries the unwind range but no section-name/flags word).
const _: () = assert!(std::mem::size_of::<InputSection>() == 56);

const IS_ALIVE: u8 = 1 << 0;
const IS_VISITED: u8 = 1 << 1;
/// Placed by the pass that synthesized it (its osec and output offset
/// are set by hand), so create_output_sections must not assign it to
/// an output section by name.
const IS_PLACED: u8 = 1 << 2;

impl InputSection {
    /// The initial flag word of a live section.
    pub fn flags_alive() -> std::sync::atomic::AtomicU8 {
        std::sync::atomic::AtomicU8::new(IS_ALIVE)
    }
    /// The initial flag word of a section that never joins the link.
    pub fn flags_dead() -> std::sync::atomic::AtomicU8 {
        std::sync::atomic::AtomicU8::new(0)
    }
    /// The initial flag word of a live synthetic section placed by the
    /// pass that made it.
    pub fn flags_placed() -> std::sync::atomic::AtomicU8 {
        std::sync::atomic::AtomicU8::new(IS_ALIVE | IS_PLACED)
    }
    /// The chunk this subsection is laid out in, once assigned.
    #[inline]
    pub fn output_section(&self) -> Option<ChunkId> {
        if self.output_section == u32::MAX {
            None
        } else {
            Some(ChunkId::unpack(self.output_section))
        }
    }

    pub fn set_output_section(&mut self, id: ChunkId) {
        self.output_section = id.pack();
    }

    #[inline]
    pub fn is_placed(&self) -> bool {
        self.flags.load(std::sync::atomic::Ordering::Relaxed) & IS_PLACED != 0
    }
    #[inline]
    /// The next output offset at or after `off` where this subsection
    /// may start. ld64 keeps each atom at the offset it had within its
    /// input section modulo the section's alignment (an 8-byte atom at
    /// offset 8 of a 16-aligned section stays 8 mod 16), rather than
    /// rounding every atom up to the section's alignment; the latter
    /// pads the output by an average of half the alignment per atom
    /// (NetNewsWire's __TEXT,__const was 11KB larger than ld-prime's).
    pub fn align_offset(&self, off: u64) -> u64 {
        let align = 1u64 << self.p2align;
        crate::macho::util::align_to_mod(off, align, self.input_addr as u64 & (align - 1))
    }

    pub fn is_alive(&self) -> bool {
        self.flags.load(std::sync::atomic::Ordering::Relaxed) & IS_ALIVE != 0
    }
    #[inline]
    pub fn set_alive(&mut self, v: bool) {
        let f = self.flags.get_mut();
        if v {
            *f |= IS_ALIVE;
        } else {
            *f &= !IS_ALIVE;
        }
    }
    /// Atomically sets the visited bit; true if this call set it.
    #[inline]
    pub fn mark_visited(&self) -> bool {
        self.flags.fetch_or(IS_VISITED, std::sync::atomic::Ordering::Relaxed) & IS_VISITED == 0
    }
    /// Reads and clears the visited bit.
    #[inline]
    pub fn take_visited(&mut self) -> bool {
        let f = self.flags.get_mut();
        let v = *f & IS_VISITED != 0;
        *f &= !IS_VISITED;
        v
    }

    /// This subsection's bytes. Empty for a zero-fill or empty section;
    /// otherwise the `size` bytes at `data_ptr` (which point into the
    /// mmap'd input, so they live for the whole link).
    #[inline]
    pub fn data(&self) -> &'static [u8] {
        if self.contents == 0 {
            &[]
        } else {
            // SAFETY: for a non-empty section data_ptr is the start of
            // `size` valid bytes in the leaked/mmap'd input, and every
            // such section is built with size == contents.len().
            unsafe { std::slice::from_raw_parts(self.contents as *const u8, self.size as usize) }
        }
    }
}
