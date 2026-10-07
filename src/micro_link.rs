use crate::arch::{Target, X86_64};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::ObjectFile;
use crate::input_sections::InputSection;
use std::collections::BTreeMap;
use std::sync::Mutex;
use zerocopy::byteorder::little_endian::{U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

const SEMANTIC_WORK_LIMIT: usize = 100_000;

#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct Surfaces {
    resolution: [u8; 32],
    shape: [u8; 32],
    comdat: [u8; 32],
    topology: [u8; 32],
    selective_topology: [u8; 32],
    requirements: [u8; 32],
    merge: [u8; 32],
    unwind: [u8; 32],
    debug: [u8; 32],
    metadata: [u8; 32],
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct ObjectImage {
    pub path_offset: U32,
    pub path_length: U32,
    pub sections_begin: U32,
    pub sections_count: U32,
    pub resolved_begin: U32,
    pub resolved_count: U32,
    pub surfaces: Surfaces,
    pub source_offset: U64,
    pub source_size: U64,
    pub source_digest: [u8; 32],
    pub metadata_items: U64,
    pub payload_bytes: U64,
}

impl Surfaces {
    pub fn selective_equivalent(self, previous: Self) -> bool {
        (self.topology == previous.topology || previous.selective_topology != [0; 32])
            && Self { topology: previous.topology, merge: previous.merge, ..self } == previous
    }
    pub fn merge_changed(self, previous: Self) -> bool {
        self.merge != previous.merge
    }
    pub fn topology_changed(self, previous: Self) -> bool {
        self.topology != previous.topology
    }
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct SectionImage {
    pub shndx: U32,
    pub relsec: U32,
    pub input_offset: U64,
    pub size: U64,
    pub output_offset: U64,
    pub address: U64,
    pub writable: U32,
    pub domain: U32,
    pub guard: [u8; 32],
    pub encoding: [u8; 32],
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct ObjectTopology {
    pub path_offset: U32,
    pub path_length: U32,
    pub sections_begin: U32,
    pub sections_count: U32,
    pub resolved_begin: U32,
    pub resolved_count: U32,
    pub surfaces: Surfaces,
    pub source_offset: U64,
    pub source_size: U64,
    pub metadata_items: U64,
    pub payload_bytes: U64,
}
impl ObjectImage {
    pub fn topology(self) -> ObjectTopology {
        ObjectTopology {
            path_offset: self.path_offset,
            path_length: self.path_length,
            sections_begin: self.sections_begin,
            sections_count: self.sections_count,
            resolved_begin: self.resolved_begin,
            resolved_count: self.resolved_count,
            surfaces: self.surfaces,
            source_offset: self.source_offset,
            source_size: self.source_size,
            metadata_items: self.metadata_items,
            payload_bytes: self.payload_bytes,
        }
    }
    pub fn from_topology(t: ObjectTopology, source_digest: [u8; 32]) -> Self {
        Self {
            path_offset: t.path_offset,
            path_length: t.path_length,
            sections_begin: t.sections_begin,
            sections_count: t.sections_count,
            resolved_begin: t.resolved_begin,
            resolved_count: t.resolved_count,
            surfaces: t.surfaces,
            source_offset: t.source_offset,
            source_size: t.source_size,
            metadata_items: t.metadata_items,
            payload_bytes: t.payload_bytes,
            source_digest,
        }
    }
}
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct SectionTopology {
    pub shndx: U32,
    pub relsec: U32,
    pub input_offset: U64,
    pub size: U64,
    pub output_offset: U64,
    pub address: U64,
    pub writable: U32,
    pub domain: U32,
}
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct SectionRevision {
    pub payload: [u8; 32],
    pub encoding: [u8; 32],
}
impl SectionImage {
    pub fn topology(self) -> SectionTopology {
        SectionTopology {
            shndx: self.shndx,
            relsec: self.relsec,
            input_offset: self.input_offset,
            size: self.size,
            output_offset: self.output_offset,
            address: self.address,
            writable: self.writable,
            domain: self.domain,
        }
    }
    pub fn revision(self) -> SectionRevision {
        SectionRevision { payload: self.guard, encoding: self.encoding }
    }
    pub fn from_topology(t: SectionTopology, r: SectionRevision) -> Self {
        Self {
            shndx: t.shndx,
            relsec: t.relsec,
            input_offset: t.input_offset,
            size: t.size,
            output_offset: t.output_offset,
            address: t.address,
            writable: t.writable,
            domain: t.domain,
            guard: r.payload,
            encoding: r.encoding,
        }
    }
}
const _: () = {
    assert!(size_of::<ObjectTopology>() == 376);
    assert!(size_of::<SectionTopology>() == 48);
    assert!(size_of::<SectionRevision>() == 64);
};
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct ResolvedValue {
    pub index: U32,
    pub reserved: U32,
    pub value: U64,
    pub key: [u8; 32],
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct BuildIdImage {
    pub offset: U64,
    pub size: U64,
}

const _: () = {
    assert!(size_of::<ObjectImage>() == 408);
    assert!(size_of::<SectionImage>() == 112);
    assert!(size_of::<ResolvedValue>() == 48);
    assert!(size_of::<BuildIdImage>() == 16);
};

struct Builder {
    surfaces: Surfaces,
    source_size: u64,
    source_digest: [u8; 32],
    cost: ObjectCost,
    sections: BTreeMap<u32, SectionImage>,
    resolved: BTreeMap<(u32, u32), ResolvedValue>,
    guards: BTreeMap<u32, ([u8; 32], [u8; 32])>,
    merge: Option<Vec<crate::merge_image::Contribution>>,
    dependencies: BTreeMap<u32, BTreeMap<crate::semantic_cells::Key, bool>>,
}

static DUPLICATES: Mutex<std::collections::BTreeSet<(std::path::PathBuf, u64)>> =
    Mutex::new(std::collections::BTreeSet::new());

static IMAGES: Mutex<BTreeMap<(std::path::PathBuf, u64), Builder>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Copy, Default)]
pub(crate) struct ObjectCost {
    pub file_bytes: usize,
    pub sections: usize,
    pub symbols: usize,
    pub relocations: usize,
    pub merge_bytes: usize,
    pub payload_bytes: usize,
    pub compressed_bytes: usize,
}
impl ObjectCost {
    fn measure(data: &[u8], headers: &[ElfShdr<X86_64>]) -> Option<Self> {
        let mut cost = Self { file_bytes: data.len(), sections: headers.len(), ..Self::default() };
        for h in headers {
            let size = usize::try_from(h.sh_size.get()).ok()?;
            match h.sh_type.get() {
                SHT_SYMTAB => cost.symbols = cost.symbols.checked_add(size / 24)?,
                SHT_RELA => cost.relocations = cost.relocations.checked_add(size / 24)?,
                SHT_CREL => return None,
                SHT_PROGBITS => {
                    cost.payload_bytes = cost.payload_bytes.checked_add(size)?;
                    if h.sh_flags.get() & SHF_MERGE as u64 != 0 {
                        cost.merge_bytes = cost.merge_bytes.checked_add(size)?;
                    }
                    if h.sh_flags.get() & SHF_COMPRESSED as u64 != 0 {
                        cost.compressed_bytes = cost.compressed_bytes.checked_add(size)?;
                    }
                }
                _ => {}
            }
        }
        Some(cost)
    }
}

pub(crate) struct RawObject<'a> {
    pub data: &'a [u8],
    pub headers: &'a [ElfShdr<X86_64>],
    pub surfaces: Surfaces,
    pub cost: ObjectCost,
    symbols: &'a [Elf64Sym<X86_64>],
    symbol_strings: &'a [u8],
}

impl<'a> RawObject<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let (header, _) = ElfEhdr::<X86_64>::ref_from_prefix(data).ok()?;
        if &header.e_ident[..7] != b"\x7fELF\x02\x01\x01"
            || header.e_machine.get() != EM_X86_64 as u16
            || header.e_type.get() != ET_REL as u16
            || header.e_shnum.get() == 0
            || header.e_shentsize.get() as usize != size_of::<ElfShdr<X86_64>>()
        {
            return None;
        }
        let start = usize::try_from(header.e_shoff.get()).ok()?;
        let size = usize::from(header.e_shnum.get()).checked_mul(size_of::<ElfShdr<X86_64>>())?;
        let headers =
            <[ElfShdr<X86_64>]>::ref_from_bytes(data.get(start..start.checked_add(size)?)?).ok()?;
        let cost = ObjectCost::measure(data, headers)?;
        if cost.sections.checked_add(cost.symbols)?.checked_add(cost.relocations)?
            > SEMANTIC_WORK_LIMIT
        {
            return None;
        }
        let symbol_header = headers.iter().find(|h| h.sh_type.get() == SHT_SYMTAB)?;
        let symbols = <[Elf64Sym<X86_64>]>::ref_from_bytes(contents(data, symbol_header)?).ok()?;
        let symbol_strings = contents(data, headers.get(symbol_header.sh_link.get() as usize)?)?;
        let names = contents(data, headers.get(header.e_shstrndx.get() as usize)?)?;
        let mut resolution = blake3::Hasher::new();
        let mut shape = blake3::Hasher::new();
        let mut comdat = blake3::Hasher::new();
        let mut topology = blake3::Hasher::new();
        let mut selective_topology = blake3::Hasher::new();
        let mut requirements = blake3::Hasher::new();
        let mut merge = blake3::Hasher::new();
        let mut unwind = blake3::Hasher::new();
        let mut debug = blake3::Hasher::new();
        let mut metadata = blake3::Hasher::new();
        shape.update(header.as_bytes());
        shape.update(headers.as_bytes());
        for h in headers {
            let name = cstring(names, h.sh_name.get() as usize)?;
            if name.starts_with(b".debug") {
                debug.update(h.as_bytes());
            }
            if h.sh_type.get() == SHT_NOBITS {
                continue;
            }
            let bytes = contents(data, h)?;
            if crate::incremental::icf_tracking()
                && h.sh_flags.get() & SHF_ALLOC as u64 != 0
                && (h.sh_flags.get() & SHF_EXECINSTR as u64 != 0
                    || h.sh_flags.get() & SHF_WRITE as u64 == 0
                    || name.starts_with(b".data.rel.ro")
                    || name.starts_with(b".gcc_except_table"))
            {
                metadata.update(bytes);
            }
            match h.sh_type.get() {
                SHT_RELA => {
                    let target = headers.get(h.sh_info.get() as usize)?;
                    let target_name = cstring(names, target.sh_name.get() as usize)?;
                    if (crate::incremental::icf_tracking()
                        && target.sh_flags.get() & SHF_ALLOC as u64 != 0)
                        || target.sh_type.get() != SHT_PROGBITS
                        || target.sh_flags.get() & (SHF_MERGE as u64 | SHF_COMPRESSED as u64) != 0
                        || target_name.starts_with(b".eh_frame")
                        || target_name.starts_with(b".sframe")
                        || target_name.starts_with(b".debug_macro")
                    {
                        metadata.update(bytes);
                    }
                    let rels = <[ElfRela<X86_64>]>::ref_from_bytes(bytes).ok()?;
                    for chunk in rels.chunks(128) {
                        let mut records = [0; 128 * 16];
                        for (record, rel) in records.chunks_exact_mut(16).zip(chunk) {
                            record.copy_from_slice(&rel.as_bytes()[..16]);
                        }
                        let records = &mut records[..chunk.len() * 16];
                        topology.update(records);
                        if cost.relocations <= 4096 {
                            if target.sh_flags.get() & SHF_ALLOC as u64 != 0 {
                                for (record, rel) in records.chunks_exact_mut(16).zip(chunk) {
                                    if rel.r_type() == R_X86_64_PC32 {
                                        record[12..16].fill(0);
                                    }
                                }
                            }
                            selective_topology.update(records);
                        }
                    }
                }
                SHT_SYMTAB => {
                    let symbols = <[Elf64Sym<X86_64>]>::ref_from_bytes(bytes).ok()?;
                    let strings = contents(data, headers.get(h.sh_link.get() as usize)?)?;
                    for symbol in symbols {
                        metadata.update(symbol.as_bytes());
                        if symbol.st_bind() == STB_LOCAL {
                            continue;
                        }
                        let name = cstring(strings, symbol.st_name() as usize)?;
                        resolution.update(&(name.len() as u64).to_le_bytes());
                        resolution.update(name);
                        resolution.update(&symbol.st_value().to_le_bytes());
                        resolution.update(&symbol.st_size().to_le_bytes());
                        resolution.update(&symbol.st_shndx().to_le_bytes());
                        resolution.update(
                            [symbol.st_bind(), symbol.st_type(), symbol.st_visibility()]
                                .map(u32::to_le_bytes)
                                .as_flattened(),
                        );
                    }
                }
                SHT_STRTAB => {
                    metadata.update(bytes);
                }
                SHT_GROUP => {
                    comdat.update(bytes);
                }
                SHT_PROGBITS if h.sh_flags.get() & (SHF_MERGE as u64) != 0 => {
                    merge.update(bytes);
                }
                SHT_PROGBITS if name.starts_with(b".eh_frame") || name.starts_with(b".sframe") => {
                    unwind.update(bytes);
                }
                SHT_PROGBITS
                    if h.sh_flags.get() & SHF_COMPRESSED as u64 == 0
                        && !name.starts_with(b".debug_macro") => {}
                SHT_NOTE => {
                    requirements.update(bytes);
                }
                _ => {
                    metadata.update(bytes);
                }
            }
        }
        Some(Self {
            data,
            headers,
            cost,
            symbols,
            symbol_strings,
            surfaces: Surfaces {
                resolution: *resolution.finalize().as_bytes(),
                shape: *shape.finalize().as_bytes(),
                comdat: *comdat.finalize().as_bytes(),
                topology: *topology.finalize().as_bytes(),
                selective_topology: if cost.relocations <= 4096 {
                    *selective_topology.finalize().as_bytes()
                } else {
                    [0; 32]
                },
                requirements: *requirements.finalize().as_bytes(),
                merge: *merge.finalize().as_bytes(),
                unwind: *unwind.finalize().as_bytes(),
                debug: *debug.finalize().as_bytes(),
                metadata: *metadata.finalize().as_bytes(),
            },
        })
    }

    pub fn merge_contributions(&self) -> Option<Vec<crate::merge_image::Contribution>> {
        let (header, _) = ElfEhdr::<X86_64>::ref_from_prefix(self.data).ok()?;
        let names = contents(self.data, self.headers.get(header.e_shstrndx.get() as usize)?)?;
        let mut result = Vec::new();
        for (index, h) in self.headers.iter().enumerate() {
            if h.sh_flags.get() & SHF_MERGE as u64 == 0 {
                continue;
            }
            let name = cstring(names, h.sh_name.get() as usize)?;
            if !crate::merge_image::eligible(
                name,
                h.sh_flags.get(),
                h.sh_entsize.get(),
                h.sh_addralign.get(),
            ) || h.sh_size.get() > 256 * 1024
                || self.symbols.iter().any(|s| s.st_shndx() as usize == index)
            {
                return None;
            }
            let bytes = self.section(index as u32)?;
            let mut pos = 0;
            while pos < bytes.len() {
                let end = pos
                    .checked_add(bytes.get(pos..)?.iter().position(|b| *b == 0)?)?
                    .checked_add(1)?;
                result.push(crate::merge_image::Contribution {
                    key: crate::merge_image::key(name, &bytes[pos..end]),
                    section: (index as u32).into(),
                    reserved: 0.into(),
                });
                if result.len() > 8192 {
                    return None;
                }
                pos = end;
            }
        }
        Some(result)
    }
    pub fn merge_fragment(&self, section: u32, occurrence: usize) -> Option<&[u8]> {
        let mut bytes = self.section(section)?;
        for _ in 0..occurrence {
            bytes = bytes.get(bytes.iter().position(|b| *b == 0)?.checked_add(1)?..)?;
        }
        bytes.get(..bytes.iter().position(|b| *b == 0)?.checked_add(1)?)
    }
    pub fn guard(&self, index: u32, _relsec: u32) -> Option<[u8; 32]> {
        Some(*blake3::hash(self.section(index)?).as_bytes())
    }
    pub fn encoding(&self, relsec: u32) -> Option<[u8; 32]> {
        Some(*blake3::hash(if relsec == u32::MAX { &[] } else { self.section(relsec)? }).as_bytes())
    }

    pub fn symbol(&self, index: u32) -> Option<&Elf64Sym<X86_64>> {
        self.symbols.get(index as usize)
    }
    pub fn symbol_key(&self, index: u32) -> Option<[u8; 32]> {
        let symbol = self.symbol(index)?;
        Some(*blake3::hash(cstring(self.symbol_strings, symbol.st_name() as usize)?).as_bytes())
    }

    pub fn section(&self, index: u32) -> Option<&'a [u8]> {
        contents(self.data, self.headers.get(index as usize)?)
    }
    pub fn rels(&self, index: u32) -> Option<&'a [ElfRela<X86_64>]> {
        if index == u32::MAX {
            return Some(&[]);
        }
        <[ElfRela<X86_64>]>::ref_from_bytes(self.section(index)?).ok()
    }
}

fn contents<'a>(data: &'a [u8], header: &ElfShdr<X86_64>) -> Option<&'a [u8]> {
    let start = usize::try_from(header.sh_offset.get()).ok()?;
    let end = start.checked_add(usize::try_from(header.sh_size.get()).ok()?)?;
    data.get(start..end)
}

fn cstring(data: &[u8], offset: usize) -> Option<&[u8]> {
    let data = data.get(offset..)?;
    data.get(..data.iter().position(|&b| b == 0)?)
}

pub(crate) fn capture_object<E: Target>(file: &ObjectFile<E>) -> Option<(std::path::PathBuf, u64)> {
    if E::NAME != "x86_64" || !crate::incremental::semantic_tracking() {
        return None;
    }
    let mf = file.base.mf.filter(|mf| {
        mf.parent.is_none_or(|p| {
            std::path::absolute(&p.name)
                .ok()
                .is_some_and(|path| ARCHIVES.lock().unwrap().contains_key(&path))
        })
    })?;
    let raw = RawObject::parse(mf.data())?;
    let root = mf.parent.unwrap_or(mf);
    let Ok(path) = std::path::absolute(&root.name) else { return None };
    let key = (path, mf.offset() as u64);
    if DUPLICATES.lock().unwrap().contains(&key) {
        return None;
    }
    let mut relsecs = vec![u32::MAX; raw.headers.len()];
    for (index, h) in raw.headers.iter().enumerate() {
        if matches!(h.sh_type.get(), SHT_RELA | SHT_CREL) {
            *relsecs.get_mut(h.sh_info.get() as usize)? = index as u32;
        }
    }
    let mut guards = BTreeMap::new();
    for (index, _) in
        raw.headers.iter().enumerate().filter(|(_, h)| h.sh_type.get() == SHT_PROGBITS)
    {
        let relsec = relsecs[index];
        guards.insert(index as u32, (raw.guard(index as u32, relsec)?, raw.encoding(relsec)?));
    }
    let mut images = IMAGES.lock().unwrap();
    if images.remove(&key).is_some() {
        DUPLICATES.lock().unwrap().insert(key);
        return None;
    }
    images.insert(
        key.clone(),
        Builder {
            surfaces: raw.surfaces,
            cost: raw.cost,
            source_size: mf.data().len() as u64,
            source_digest: if mf.parent.is_some() {
                *blake3::hash(mf.data()).as_bytes()
            } else {
                [0; 32]
            },
            sections: BTreeMap::new(),
            resolved: BTreeMap::new(),
            guards,
            merge: raw.merge_contributions(),
            dependencies: BTreeMap::new(),
        },
    );
    Some(key)
}

pub(crate) fn capture_section<E: Target>(
    ctx: &Context<E>,
    isec: &InputSection<E>,
    output_offset: u64,
) {
    if !crate::incremental::semantic_tracking() || !crate::incremental::delta_eligible(&ctx.args) {
        return;
    }
    let file = &ctx.objs[isec.file.index()];
    let Some(key) = file.incremental_image.as_ref() else { return };
    let Some(mf) = file.base.mf else { return };
    if isec.shndx as usize >= file.num_elf_sections {
        return;
    }
    let original = file.shdr(isec.shndx as usize);
    if original.sh_type.get() != SHT_PROGBITS {
        return;
    }
    let raw_source = &mf.data()[original.sh_offset.get() as usize
        ..(original.sh_offset.get() + original.sh_size.get()) as usize];
    let mut writable = !isec.is_compressed()
        && isec.sh_size == original.sh_size.get()
        && isec.contents().as_ptr() == raw_source.as_ptr();
    let mut resolved = Vec::new();
    let mut selective = isec.is_alloc() && !ctx.args.gc_sections && !ctx.args.icf;
    for rel in isec.rels(file) {
        if rel.r_type() == R_NONE {
            continue;
        }
        let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
        selective &=
            file.base.elf_syms.get(rel.r_sym() as usize).is_some_and(|s| s.st_bind() != STB_LOCAL)
                && rel.r_type() == R_X86_64_PC32
                && !sym.is_absolute()
                && sym.is_pcrel_linktime_const(ctx)
                && sym.ty() != STT_TLS;
        let relax = matches!(
            rel.r_type(),
            R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX | R_X86_64_CODE_4_GOTPCRELX
        );
        if (crate::arch::x86_64::relocation_cell(rel.r_type()).is_none() && !relax)
            || (isec.sh_flags & SHF_ALLOC as u64 == 0
                && !matches!(rel.r_type(), R_X86_64_64 | R_X86_64_32 | R_X86_64_32S))
            || (sym.is_imported()
                && !matches!(
                    rel.r_type(),
                    R_X86_64_PLT32
                        | R_X86_64_GOTPCREL
                        | R_X86_64_GOTPCREL64
                        | R_X86_64_GOTPCRELX
                        | R_X86_64_REX_GOTPCRELX
                        | R_X86_64_CODE_4_GOTPCRELX
                ))
            || (sym.ty() == STT_TLS
                && !matches!(
                    rel.r_type(),
                    R_X86_64_TPOFF32
                        | R_X86_64_TPOFF64
                        | R_X86_64_DTPOFF32
                        | R_X86_64_DTPOFF64
                        | R_X86_64_CODE_6_GOTTPOFF
                ))
            || sym.is_ifunc()
            || sym.is_remaining_undef_weak()
            || sym.is_fragment_dummy()
            || (sym.input_section_ref(ctx).is_none() && !sym.is_absolute() && !sym.is_imported())
            || sym.input_section_ref(ctx).is_some_and(|s| !s.is_alive())
            || (isec.sh_flags & SHF_ALLOC as u64 != 0 && rel.r_type() == R_X86_64_64)
            || (isec.sh_flags & SHF_ALLOC as u64 != 0
                && rel.r_type() == R_X86_64_32
                && sym.is_imported())
        {
            writable = false;
            break;
        }
        let cell = crate::arch::x86_64::relocation_cell(rel.r_type())
            .unwrap_or(crate::reloc_env::RelocCell::Got);
        let local = file.base.elf_syms[rel.r_sym() as usize].st_bind() == STB_LOCAL;
        if local
            && cell == crate::reloc_env::RelocCell::Symbol
            && sym.input_section_ref(ctx).is_some_and(|s| s.is_alive())
        {
            continue;
        }
        let value = match cell {
            crate::reloc_env::RelocCell::Symbol => sym.addr(ctx),
            crate::reloc_env::RelocCell::Got => sym.got_addr(ctx),
            crate::reloc_env::RelocCell::GotTp => sym.gottp_addr(ctx),
            crate::reloc_env::RelocCell::TpOffset => sym.addr(ctx).wrapping_sub(ctx.tp_addr),
            crate::reloc_env::RelocCell::DtpOffset => sym.addr(ctx).wrapping_sub(ctx.dtp_addr),
            crate::reloc_env::RelocCell::RelaxPredicate => unreachable!(),
        };
        let name_offset = file.base.elf_syms[rel.r_sym() as usize].st_name() as usize;
        let Some(name) = cstring(file.base.symbol_strtab, name_offset) else {
            writable = false;
            break;
        };
        let key = *blake3::hash(name).as_bytes();
        if relax {
            resolved.push(ResolvedValue {
                index: rel.r_sym().into(),
                reserved: (crate::reloc_env::RelocCell::Symbol as u32).into(),
                value: sym.addr(ctx).into(),
                key,
            });
            resolved.push(ResolvedValue {
                index: rel.r_sym().into(),
                reserved: (crate::reloc_env::RelocCell::RelaxPredicate as u32).into(),
                value: u64::from(sym.is_pcrel_linktime_const(ctx)).into(),
                key,
            });
        }
        resolved.push(ResolvedValue {
            index: rel.r_sym().into(),
            reserved: (cell as u32).into(),
            value: value.into(),
            key,
        });
    }
    let raw_header = file.shdr(isec.shndx as usize);
    let mut images = IMAGES.lock().unwrap();
    let Some(image) = images.get_mut(key) else { return };
    let Some((guard, encoding)) = image.guards.get(&isec.shndx).copied() else { return };
    if writable {
        image.dependencies.insert(
            isec.shndx,
            resolved
                .iter()
                .map(|v| {
                    let dependency =
                        if file.base.elf_syms[v.index.get() as usize].st_bind() == STB_LOCAL {
                            let mut hash = blake3::Hasher::new();
                            hash.update(b"mold-local-cell");
                            let path = key.0.as_os_str().as_encoded_bytes();
                            hash.update(&(path.len() as u64).to_le_bytes());
                            hash.update(path);
                            hash.update(&key.1.to_le_bytes());
                            hash.update(v.index.as_bytes());
                            hash.update(&v.key);
                            *hash.finalize().as_bytes()
                        } else {
                            v.key
                        };
                    let symbol = &ctx.symbols[file.base.symbols[v.index.get() as usize]];
                    let proof = v.reserved.get() == crate::reloc_env::RelocCell::Symbol as u32
                        && !symbol.is_absolute()
                        && symbol.ty() != STT_TLS
                        && symbol.is_pcrel_linktime_const(ctx);
                    ((dependency, v.reserved.get()), proof)
                })
                .collect(),
        );
        for value in resolved {
            image.resolved.insert((value.index.get(), value.reserved.get()), value);
        }
    }
    image.sections.insert(
        isec.shndx,
        SectionImage {
            shndx: isec.shndx.into(),
            relsec: isec.relsec_idx().unwrap_or(u32::MAX).into(),
            input_offset: raw_header.sh_offset.get().into(),
            size: isec.sh_size.into(),
            output_offset: output_offset.into(),
            address: isec.addr(ctx).into(),
            writable: (u32::from(writable) | (u32::from(writable && selective) << 1)).into(),
            domain: (if isec.sh_flags & SHF_ALLOC as u64 != 0 {
                0u32
            } else if isec.name(file).starts_with(b".debug") {
                1u32
            } else {
                2u32
            })
            .into(),
            guard,
            encoding,
        },
    );
}

pub(crate) fn take_images(
    strings: &mut Vec<u8>,
) -> (Vec<ObjectImage>, Vec<SectionImage>, Vec<ResolvedValue>, Vec<ArchiveImage>) {
    let images = std::mem::take(&mut *IMAGES.lock().unwrap());
    DUPLICATES.lock().unwrap().clear();
    let mut objects = Vec::with_capacity(images.len());
    let mut sections = Vec::new();
    let mut resolved = Vec::new();
    let mut dependency_sections = Vec::new();
    let mut merge_links = Vec::new();
    let mut merge_uses = Vec::new();
    for ((path, source_offset), mut image) in images {
        merge_links.push((
            merge_uses.len() as u32,
            image.merge.as_ref().map_or(u32::MAX, |m| m.len() as u32),
        ));
        if let Some(m) = image.merge {
            merge_uses.extend(m);
        }
        let bytes = path.as_os_str().as_encoded_bytes();
        let object = ObjectImage {
            path_offset: (strings.len() as u32).into(),
            path_length: (bytes.len() as u32).into(),
            sections_begin: (sections.len() as u32).into(),
            sections_count: (image.sections.len() as u32).into(),
            resolved_begin: (resolved.len() as u32).into(),
            resolved_count: (image.resolved.len() as u32).into(),
            surfaces: image.surfaces,
            source_offset: source_offset.into(),
            source_size: image.source_size.into(),
            source_digest: image.source_digest,
            metadata_items: ((image.cost.sections + image.cost.symbols + image.cost.relocations)
                as u64)
                .into(),
            payload_bytes: (image.cost.payload_bytes as u64).into(),
        };
        strings.extend_from_slice(bytes);
        for section in image.sections.into_values() {
            dependency_sections
                .push(image.dependencies.remove(&section.shndx.get()).unwrap_or_default());
            sections.push(section);
        }
        resolved.extend(image.resolved.into_values());
        objects.push(object);
    }
    let mut archives = Vec::new();
    for (path, (guard, count)) in std::mem::take(&mut *ARCHIVES.lock().unwrap()) {
        let bytes = path.as_os_str().as_encoded_bytes();
        archives.push(ArchiveImage {
            path_offset: (strings.len() as u32).into(),
            path_length: (bytes.len() as u32).into(),
            member_count: count.into(),
            reserved: 0.into(),
            guard,
        });
        strings.extend_from_slice(bytes);
    }
    crate::semantic_cells::capture(dependency_sections);
    *MERGE_USES.lock().unwrap() = Some((merge_links, merge_uses));
    (objects, sections, resolved, archives)
}
static MERGE_USES: Mutex<Option<MergeUses>> = Mutex::new(None);
type MergeUses = (Vec<(u32, u32)>, Vec<crate::merge_image::Contribution>);
pub(crate) fn take_merge_uses() -> Option<MergeUses> {
    MERGE_USES.lock().unwrap().take()
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct ArchiveImage {
    pub path_offset: U32,
    pub path_length: U32,
    pub member_count: U32,
    pub reserved: U32,
    pub guard: [u8; 32],
}
static ARCHIVES: Mutex<BTreeMap<std::path::PathBuf, ([u8; 32], u32)>> = Mutex::new(BTreeMap::new());
pub(crate) fn archive_surface(data: &[u8]) -> Option<([u8; 32], u32)> {
    let thin = data.starts_with(b"!<thin>\n");
    if !thin && !data.starts_with(b"!<arch>\n") {
        return None;
    }
    let mut hash = blake3::Hasher::new();
    hash.update(&data[..8]);
    let mut offset = 8;
    let mut count = 0u32;
    let mut work = 0usize;
    while offset < data.len() {
        let header = data.get(offset..offset.checked_add(60)?)?;
        if &header[58..] != b"`\n" {
            return None;
        }
        let length = std::str::from_utf8(&header[48..58]).ok()?.trim().parse::<usize>().ok()?;
        hash.update(&(offset as u64).to_le_bytes());
        hash.update(&header[..16]);
        hash.update(&header[48..]);
        offset = offset.checked_add(60)?;
        if thin
            && !header[..16].starts_with(b"/ ")
            && !header[..16].starts_with(b"// ")
            && !header[..16].starts_with(b"/SYM64/ ")
        {
            count = count.checked_add(1)?;
            continue;
        }
        let end = offset.checked_add(length)?;
        let body = data.get(offset..end)?;
        if body.starts_with(b"\x7fELF") {
            count = count.checked_add(1)?;
            let (eh, _) = ElfEhdr::<X86_64>::ref_from_prefix(body).ok()?;
            let start = usize::try_from(eh.e_shoff.get()).ok()?;
            let end = start.checked_add(usize::from(eh.e_shnum.get()).checked_mul(64)?)?;
            let headers = <[ElfShdr<X86_64>]>::ref_from_bytes(body.get(start..end)?).ok()?;
            let cost = ObjectCost::measure(body, headers)?;
            work = work
                .checked_add(cost.sections)?
                .checked_add(cost.symbols)?
                .checked_add(cost.relocations)?;
            if work > 20_000 {
                return None;
            }
        } else {
            hash.update(body);
        }
        offset = end.checked_add(length % 2)?;
    }
    if offset != data.len() {
        return None;
    }
    Some((*hash.finalize().as_bytes(), count))
}
pub(crate) fn capture_archive(mf: &crate::mapped_file::MappedFile) {
    if crate::incremental::semantic_tracking()
        && let Some(surface) = archive_surface(mf.data())
        && let Ok(path) = std::path::absolute(&mf.name)
    {
        ARCHIVES.lock().unwrap().insert(path, surface);
    }
}
