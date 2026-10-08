//! `.note.gnu.property`, merged ISA properties.

use std::collections::{BTreeMap, BTreeSet};

use crate::arch::{Family, Target};
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;

// .note.gnu.property section contains additional runtime information
// about ISA variant.
#[derive(Debug)]
pub struct NotePropertySection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub contents: Vec<(u32, u32)>,
}

impl<E: Target> NotePropertySection<E> {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::<E>::new(".note.gnu.property", SHT_NOTE, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        Self { hdr, contents: Vec::new() }
    }

    /// The merged value of a property, or 0 if the output doesn't have it.
    pub fn get(&self, ty: u32) -> u32 {
        self.contents.iter().find(|&&(key, _)| key == ty).map_or(0, |&(_, val)| val)
    }
}

impl<E: Target> Default for NotePropertySection<E> {
    fn default() -> Self {
        Self::new()
    }
}

fn entry_size<E: Target>() -> usize {
    if E::IS_64 { 16 } else { 12 }
}

/// Whether the output is marked as compatible with ARM64 Branch Target
/// Identification (BTI). The code of such a file is mapped to guarded pages
/// in which an indirect branch must land on a `bti` instruction. Therefore,
/// linker-synthesized code that may be reached by an indirect branch has
/// to start with one.
pub fn is_bti<E: Target>(ctx: &Context<E>) -> bool {
    E::FAMILY == Family::Arm64
        && ctx.note_property.as_ref().is_some_and(|sec| {
            sec.get(GNU_PROPERTY_AARCH64_FEATURE_1_AND) & GNU_PROPERTY_AARCH64_FEATURE_1_BTI != 0
        })
}

// Merges input files' .note.gnu.property values. This has to be done
// before computing section sizes because the result affects the PLT
// format on ARM64.
pub fn construct<E: Target>(ctx: &mut Context<E>) {
    // Obtain the list of keys
    let files: Vec<&crate::input_files::ObjectFile<E>> =
        ctx.objs.iter().filter(|file| !ctx.is_internal(file.id())).collect();
    let keys: BTreeSet<u32> = files.iter().flat_map(|f| f.gnu_properties.keys().copied()).collect();
    let value = |f: &crate::input_files::ObjectFile<E>, key: u32| {
        f.gnu_properties.get(&key).copied().unwrap_or(0)
    };

    // Merge values for each key
    let mut map: BTreeMap<u32, u32> = BTreeMap::new();
    for key in keys {
        if (E::IS_X86
            && (GNU_PROPERTY_X86_UINT32_AND_LO..=GNU_PROPERTY_X86_UINT32_AND_HI).contains(&key))
            || (E::FAMILY == Family::Arm64 && key == GNU_PROPERTY_AARCH64_FEATURE_1_AND)
        {
            // An AND feature is set if all input objects have the property and
            // the feature.
            map.insert(key, files.iter().fold(u32::MAX, |acc, f| acc & value(f, key)));
        } else if E::IS_X86
            && (GNU_PROPERTY_X86_UINT32_OR_LO..=GNU_PROPERTY_X86_UINT32_OR_HI).contains(&key)
        {
            // An OR feature is set if some input object has the feature.
            map.insert(key, files.iter().fold(0, |acc, f| acc | value(f, key)));
        } else if E::IS_X86
            && (GNU_PROPERTY_X86_UINT32_OR_AND_LO..=GNU_PROPERTY_X86_UINT32_OR_AND_HI)
                .contains(&key)
        {
            // An OR-AND feature is set if all input object files have the property
            // and some of them have the feature.
            if files.iter().all(|f| f.gnu_properties.contains_key(&key)) {
                map.insert(key, files.iter().fold(0, |acc, f| acc | value(f, key)));
            }
        }
    }

    if E::IS_X86 {
        if ctx.args.z_ibt {
            *map.entry(GNU_PROPERTY_X86_FEATURE_1_AND).or_insert(0) |=
                GNU_PROPERTY_X86_FEATURE_1_IBT;
        }
        if ctx.args.z_shstk {
            *map.entry(GNU_PROPERTY_X86_FEATURE_1_AND).or_insert(0) |=
                GNU_PROPERTY_X86_FEATURE_1_SHSTK;
        }
        *map.entry(GNU_PROPERTY_X86_ISA_1_NEEDED).or_insert(0) |= ctx.args.z_x86_64_isa_level;
    }

    if E::FAMILY == Family::Arm64 {
        let features = map.entry(GNU_PROPERTY_AARCH64_FEATURE_1_AND).or_insert(0);
        if ctx.args.z_force_bti {
            *features |= GNU_PROPERTY_AARCH64_FEATURE_1_BTI;
        }
    }

    ctx.note_property.as_mut().unwrap().contents =
        map.into_iter().filter(|&(_, v)| v != 0).collect();
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    let sec = ctx.note_property.as_mut().unwrap();
    let n = sec.contents.len();
    sec.hdr.shdr.sh_size.set(if n == 0 { 0 } else { (16 + n * entry_size::<E>()) as u64 });
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = ctx.note_property.as_ref().unwrap();
    buf.fill(0);
    E::write_u32(buf, 4); // Name size
    E::write_u32(&mut buf[4..], sec.hdr.shdr.sh_size.get() as u32 - 16); // Content size
    E::write_u32(&mut buf[8..], NT_GNU_PROPERTY_TYPE_0);
    buf[12..16].copy_from_slice(b"GNU\0");
    for (i, &(ty, val)) in sec.contents.iter().enumerate() {
        let off = 16 + i * entry_size::<E>();
        E::write_u32(&mut buf[off..], ty);
        E::write_u32(&mut buf[off + 4..], 4);
        E::write_u32(&mut buf[off + 8..], val); // Content
    }
}
