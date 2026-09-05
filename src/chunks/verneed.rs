//! `.gnu.version_r`, required symbol versions.

use crate::arch::Arch;
use crate::chunks::dynstr::DynstrSection;
use crate::chunks::hash::elf_hash;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{DsoId, FileId};
use crate::symbol::SymbolId;
use crate::util::endian::{U16, U32};

// .gnu.version_r contains information to refer to shared libraries and
// their symbol versions.
#[derive(Debug)]
pub struct VerneedSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub contents: Vec<u8>,
}

impl<E: Layout> VerneedSection<E> {
    pub fn new() -> VerneedSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".gnu.version_r", SHT_GNU_VERNEED, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(4);
        VerneedSection {
            hdr,
            contents: Vec::new(),
        }
    }
}

impl<E: Layout> Default for VerneedSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

// If `-z pack-relative-relocs` is specified, we'll create a .relr.dyn
// section and store base relocation records to that section instead of
// to the usual .rela.dyn section.
//
// .relr.dyn is relatively new feature and not supported by glibc until
// 2.38 which was released in 2022. If we don't do anything, executables
// built with `-z pack-relative-relocs` would just crash immediately on
// startup with an older version of glibc.
//
// As a workaround, we'll add a dependency to a dummy version name
// "GLIBC_ABI_DT_RELR" if `-z pack-relative-relocs` is given so that
// executables built with the option failed with a more friendly "version
// `GLIBC_ABI_DT_RELR' not found" error message. glibc 2.38 or later knows
// about this dummy version name and simply ignores it.
fn is_glibc2<E: Layout>(dso: &crate::input_files::SharedFile<E>) -> bool {
    dso.soname.starts_with("libc.so.")
        && dso
            .version_strings
            .iter()
            .any(|v| v.starts_with(b"GLIBC_2."))
}

pub fn construct<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("fill_verneed");

    // Create a list of versioned symbols and sort by file and version.
    let dynsym = &ctx.dynsym;
    let mut syms: Vec<(DsoId, SymbolId)> = dynsym
        .symbols
        .iter()
        .skip(1)
        .flatten()
        .filter_map(|&id| {
            let sym = &ctx.symbols[id];
            match sym.file() {
                Some(FileId::Dso(dso)) if (sym.ver_idx as u32) > VER_NDX_LAST_RESERVED => {
                    Some((dso, id))
                }
                _ => None,
            }
        })
        .collect();
    if syms.is_empty() {
        return;
    }
    syms.sort_by_key(|&(dso, id)| {
        (
            ctx.dsos[dso.index()].soname.clone(),
            ctx.symbols[id].ver_idx,
        )
    });

    // Resize .gnu.version
    let n = dynsym.symbols.len();
    ctx.versym.contents.resize(n, VER_NDX_GLOBAL as u16);
    ctx.versym.contents[0] = VER_NDX_LOCAL as u16;

    // Allocate a large enough buffer for .gnu.version_r.
    let capacity = (ElfVerneed::<E>::size() + ElfVernaux::<E>::size()) * (syms.len() + 1);
    let mut builder = VerneedBuilder::<E> {
        contents: Vec::with_capacity(capacity),
        veridx: VER_NDX_LAST_RESERVED as u16 + ctx.args.version_definitions.len() as u16,
        num_groups: 0,
        group_pos: None,
        aux_pos: None,
        _arch: std::marker::PhantomData,
    };

    // Fill .gnu.version_r.
    // Create version entries.
    for i in 0..syms.len() {
        let (dso, id) = syms[i];
        let start_group = i == 0 || syms[i - 1].0 != dso;
        if start_group {
            let soname = ctx.dsos[dso.index()].soname.clone();
            let vn_file = ctx.dynstr.find_string(soname.as_bytes()) as u32;
            builder.start_group(vn_file);
            if ctx.args.pack_dyn_relocs_relr && is_glibc2(&ctx.dsos[dso.index()]) {
                builder.add_entry(&mut ctx.dynstr, b"GLIBC_ABI_DT_RELR");
            }
            let version = ctx.symbols[id].version(ctx);
            builder.add_entry(&mut ctx.dynstr, version);
        } else if ctx.symbols[syms[i - 1].1].ver_idx != ctx.symbols[id].ver_idx {
            let version = ctx.symbols[id].version(ctx);
            builder.add_entry(&mut ctx.dynstr, version);
        }
        let dynsym_idx = ctx.symbols[id].dynsym_idx(&ctx.symbols).unwrap() as usize;
        ctx.versym.contents[dynsym_idx] = builder.veridx;
    }

    // Resize .gnu.version_r to fit to its contents.
    ctx.verneed.hdr.shdr.sh_info.set(builder.num_groups);
    ctx.verneed.contents = builder.contents;
}

/// Builds `.gnu.version_r` incrementally: a verneed entry per shared
/// library, each followed by its vernaux entries.
struct VerneedBuilder<E: Arch> {
    contents: Vec<u8>,
    veridx: u16,
    num_groups: u32,
    /// Positions of the current group's verneed and last vernaux.
    group_pos: Option<usize>,
    aux_pos: Option<usize>,
    _arch: std::marker::PhantomData<E>,
}

impl<E: Arch> VerneedBuilder<E> {
    fn start_group(&mut self, vn_file: u32) {
        self.num_groups += 1;
        if let Some(gp) = self.group_pos {
            let mut vn = ElfVerneed::<E>::parse(&self.contents[gp..]);
            vn.vn_next.set((self.contents.len() - gp) as u32);
            vn.write(&mut self.contents[gp..]);
        }
        let pos = self.contents.len();
        self.group_pos = Some(pos);
        self.aux_pos = None;
        self.contents.resize(pos + ElfVerneed::<E>::size(), 0);
        ElfVerneed::<E> {
            vn_version: U16::new(1),
            vn_cnt: U16::default(),
            vn_file: U32::new(vn_file),
            vn_aux: U32::new(ElfVerneed::<E>::size() as u32),
            vn_next: U32::default(),
        }
        .write(&mut self.contents[pos..]);
    }

    fn add_entry(&mut self, dynstr: &mut DynstrSection<E>, verstr: &[u8]) {
        let gp = self.group_pos.unwrap();
        let mut vn = ElfVerneed::<E>::parse(&self.contents[gp..]);
        vn.vn_cnt.set(vn.vn_cnt.get() + 1);
        vn.write(&mut self.contents[gp..]);
        if let Some(ap) = self.aux_pos {
            let mut aux = ElfVernaux::<E>::parse(&self.contents[ap..]);
            aux.vna_next.set(ElfVernaux::<E>::size() as u32);
            aux.write(&mut self.contents[ap..]);
        }
        self.veridx += 1;
        let aux = ElfVernaux::<E> {
            vna_hash: U32::new(elf_hash(verstr)),
            vna_flags: U16::default(),
            vna_other: U16::new(self.veridx),
            vna_name: U32::new(dynstr.add_string(verstr) as u32),
            vna_next: U32::default(),
        };
        let pos = self.contents.len();
        self.aux_pos = Some(pos);
        self.contents.resize(pos + ElfVernaux::<E>::size(), 0);
        aux.write(&mut self.contents[pos..]);
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let size = ctx.verneed.contents.len() as u64;
    ctx.verneed.hdr.shdr.sh_size.set(size);
    ctx.verneed.hdr.shdr.sh_link.set(ctx.dynstr.hdr.shndx);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    buf[..ctx.verneed.contents.len()].copy_from_slice(&ctx.verneed.contents);
}
