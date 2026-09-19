//! `.note.gnu.property`, merged ISA properties.

use std::collections::{BTreeMap, BTreeSet};

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::util::endian::Endian;

// .note.gnu.property section contains an additional runtime information
// about ISA variant.
#[derive(Debug)]
pub struct NotePropertySection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub contents: Vec<(u32, u32)>,
}

impl<E: Arch> NotePropertySection<E> {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::<E>::new(".note.gnu.property", SHT_NOTE, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        Self { hdr, contents: Vec::new() }
    }
}

impl<E: Arch> Default for NotePropertySection<E> {
    fn default() -> Self {
        Self::new()
    }
}

fn entry_size<E: Arch>() -> usize {
    if E::IS_64 { 16 } else { 12 }
}

// Merges input files' .note.gnu.property values.
pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
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
        if (GNU_PROPERTY_X86_UINT32_AND_LO..=GNU_PROPERTY_X86_UINT32_AND_HI).contains(&key) {
            // An AND feature is set if all input objects have the property and
            // the feature.
            map.insert(key, files.iter().fold(u32::MAX, |acc, f| acc & value(f, key)));
        } else if (GNU_PROPERTY_X86_UINT32_OR_LO..=GNU_PROPERTY_X86_UINT32_OR_HI).contains(&key) {
            // An OR feature is set if some input object has the feature.
            map.insert(key, files.iter().fold(0, |acc, f| acc | value(f, key)));
        } else if (GNU_PROPERTY_X86_UINT32_OR_AND_LO..=GNU_PROPERTY_X86_UINT32_OR_AND_HI)
            .contains(&key)
        {
            // An OR-AND feature is set if all input object files have the property
            // and some of them has the feature.
            if files.iter().all(|f| f.gnu_properties.contains_key(&key)) {
                map.insert(key, files.iter().fold(0, |acc, f| acc | value(f, key)));
            }
        }
    }

    if ctx.args.z_ibt {
        *map.entry(GNU_PROPERTY_X86_FEATURE_1_AND).or_insert(0) |= GNU_PROPERTY_X86_FEATURE_1_IBT;
    }
    if ctx.args.z_shstk {
        *map.entry(GNU_PROPERTY_X86_FEATURE_1_AND).or_insert(0) |= GNU_PROPERTY_X86_FEATURE_1_SHSTK;
    }
    *map.entry(GNU_PROPERTY_X86_ISA_1_NEEDED).or_insert(0) |= ctx.args.z_x86_64_isa_level;

    // Serialize the map
    let contents: Vec<(u32, u32)> = map.into_iter().filter(|&(_, v)| v != 0).collect();
    let sec = ctx.note_property.as_mut().unwrap();
    sec.hdr.shdr.sh_size.set(if contents.is_empty() {
        0
    } else {
        (16 + contents.len() * entry_size::<E>()) as u64
    });
    sec.contents = contents;
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = ctx.note_property.as_ref().unwrap();
    buf.fill(0);
    E::Endian::write_u32(buf, 4); // Name size
    E::Endian::write_u32(&mut buf[4..], sec.hdr.shdr.sh_size.get() as u32 - 16); // Content size
    E::Endian::write_u32(&mut buf[8..], NT_GNU_PROPERTY_TYPE_0);
    buf[12..16].copy_from_slice(b"GNU\0");
    for (i, &(ty, val)) in sec.contents.iter().enumerate() {
        let off = 16 + i * entry_size::<E>();
        E::Endian::write_u32(&mut buf[off..], ty);
        E::Endian::write_u32(&mut buf[off + 4..], 4);
        E::Endian::write_u32(&mut buf[off + 8..], val); // Content
    }
}
