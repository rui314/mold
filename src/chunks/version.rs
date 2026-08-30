//! Symbol versioning sections: `.gnu.version`, `.gnu.version_r` and
//! `.gnu.version_d`.

use crate::arch::Arch;
use crate::chunks::symtab::elf_hash;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{DsoId, FileId};
use crate::symbol::SymbolId;
use crate::util::path_filename;

/// `.gnu.version` is a table parallel to `.dynsym` giving each symbol's
/// version index.
#[derive(Debug)]
pub struct VersymSection {
    pub hdr: ChunkHeader,
    pub contents: Vec<u16>,
}

impl VersymSection {
    pub fn new() -> VersymSection {
        let mut hdr = ChunkHeader::new(".gnu.version", SHT_GNU_VERSYM, SHF_ALLOC as u64);
        hdr.shdr.sh_entsize = 2;
        hdr.shdr.sh_addralign = 2;
        VersymSection {
            hdr,
            contents: Vec::new(),
        }
    }
}

impl Default for VersymSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod versym {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        ctx.versym.hdr.shdr.sh_size = ctx.versym.contents.len() as u64 * 2;
        ctx.versym.hdr.shdr.sh_link = ctx.dynsym.hdr.shndx;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        for (i, &v) in ctx.versym.contents.iter().enumerate() {
            E::Endian::write_u16(&mut buf[i * 2..], v);
        }
    }
}

/// `.gnu.version_r` lists the versions required from each shared library.
#[derive(Debug)]
pub struct VerneedSection {
    pub hdr: ChunkHeader,
    pub contents: Vec<u8>,
}

impl VerneedSection {
    pub fn new() -> VerneedSection {
        let mut hdr = ChunkHeader::new(".gnu.version_r", SHT_GNU_VERNEED, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign = 4;
        VerneedSection {
            hdr,
            contents: Vec::new(),
        }
    }
}

impl Default for VerneedSection {
    fn default() -> Self {
        Self::new()
    }
}

/// `-z pack-relative-relocs` outputs crash on glibc before 2.38, which
/// doesn't understand RELR. Adding a dependency on the dummy version
/// `GLIBC_ABI_DT_RELR` turns that into a friendlier "version not found"
/// error; newer glibc knows the name and ignores it.
fn is_glibc2(dso: &crate::input_files::SharedFile) -> bool {
    dso.soname.starts_with("libc.so.")
        && dso
            .version_strings
            .iter()
            .any(|v| v.starts_with(b"GLIBC_2."))
}

pub mod verneed {
    use super::*;

    pub fn construct<E: Arch>(ctx: &mut Context<E>) {
        let _t = ctx.timer("fill_verneed");

        // Versioned symbols, sorted by file and version.
        let mut syms: Vec<(DsoId, SymbolId)> = ctx
            .dynsym
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

        let n = ctx.dynsym.symbols.len();
        ctx.versym.contents.resize(n, VER_NDX_GLOBAL as u16);
        ctx.versym.contents[0] = VER_NDX_LOCAL as u16;

        let mut builder = VerneedBuilder::<E> {
            contents: Vec::new(),
            veridx: VER_NDX_LAST_RESERVED as u16 + ctx.args.version_definitions.len() as u16,
            num_groups: 0,
            group_pos: None,
            aux_pos: None,
            _arch: std::marker::PhantomData,
        };

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

        ctx.verneed.hdr.shdr.sh_info = builder.num_groups;
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
                let mut vn = ElfVerneed::parse::<E>(&self.contents[gp..]);
                vn.vn_next = (self.contents.len() - gp) as u32;
                vn.write::<E>(&mut self.contents[gp..]);
            }
            let pos = self.contents.len();
            self.group_pos = Some(pos);
            self.aux_pos = None;
            self.contents.resize(pos + ElfVerneed::size::<E>(), 0);
            ElfVerneed {
                vn_version: 1,
                vn_cnt: 0,
                vn_file,
                vn_aux: ElfVerneed::size::<E>() as u32,
                vn_next: 0,
            }
            .write::<E>(&mut self.contents[pos..]);
        }

        fn add_entry(&mut self, dynstr: &mut super::super::symtab::DynstrSection, verstr: &[u8]) {
            let gp = self.group_pos.unwrap();
            let mut vn = ElfVerneed::parse::<E>(&self.contents[gp..]);
            vn.vn_cnt += 1;
            vn.write::<E>(&mut self.contents[gp..]);
            if let Some(ap) = self.aux_pos {
                let mut aux = ElfVernaux::parse::<E>(&self.contents[ap..]);
                aux.vna_next = ElfVernaux::size::<E>() as u32;
                aux.write::<E>(&mut self.contents[ap..]);
            }
            self.veridx += 1;
            let aux = ElfVernaux {
                vna_hash: elf_hash(verstr),
                vna_flags: 0,
                vna_other: self.veridx,
                vna_name: dynstr.add_string(verstr) as u32,
                vna_next: 0,
            };
            let pos = self.contents.len();
            self.aux_pos = Some(pos);
            self.contents.resize(pos + ElfVernaux::size::<E>(), 0);
            aux.write::<E>(&mut self.contents[pos..]);
        }
    }

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        ctx.verneed.hdr.shdr.sh_size = ctx.verneed.contents.len() as u64;
        ctx.verneed.hdr.shdr.sh_link = ctx.dynstr.hdr.shndx;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf[..ctx.verneed.contents.len()].copy_from_slice(&ctx.verneed.contents);
    }
}

/// `.gnu.version_d` defines the versions of the symbols this file
/// exports. It exists only in shared libraries.
#[derive(Debug)]
pub struct VerdefSection {
    pub hdr: ChunkHeader,
    pub contents: Vec<u8>,
}

impl VerdefSection {
    pub fn new() -> VerdefSection {
        let mut hdr = ChunkHeader::new(".gnu.version_d", SHT_GNU_VERDEF, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign = 4;
        VerdefSection {
            hdr,
            contents: Vec::new(),
        }
    }
}

impl Default for VerdefSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod verdef {
    use super::*;

    pub fn construct<E: Arch>(ctx: &mut Context<E>) {
        let _t = ctx.timer("fill_verdef");
        if ctx.args.version_definitions.is_empty() {
            return;
        }

        // --default-symver
        if ctx.args.default_symver {
            for &id in ctx.dynsym.symbols.iter().flatten() {
                let sym = &mut ctx.symbols[id];
                if matches!(sym.file(), Some(FileId::Obj(_))) && !sym.is_undef() {
                    let ver = sym.ver_idx as u32;
                    if ver == VER_NDX_GLOBAL || ver == VER_NDX_UNSPECIFIED {
                        sym.ver_idx = VER_NDX_LAST_RESERVED as u16 + 1;
                    }
                }
            }
        }

        let n = ctx.dynsym.symbols.len();
        ctx.versym.contents.resize(n, VER_NDX_GLOBAL as u16);
        ctx.versym.contents[0] = VER_NDX_LOCAL as u16;

        for &id in ctx.dynsym.symbols.iter().flatten() {
            let sym = &ctx.symbols[id];
            if !matches!(sym.file(), Some(FileId::Obj(_))) {
                continue;
            }
            let idx = sym.dynsym_idx(&ctx.symbols).unwrap() as usize;
            // An unversioned undefined symbol takes version index 0.
            if sym.ver_idx as u32 != VER_NDX_UNSPECIFIED {
                ctx.versym.contents[idx] = sym.ver_idx;
            } else if sym.is_undef() {
                ctx.versym.contents[idx] = VER_NDX_LOCAL as u16;
            }
        }

        let mut contents: Vec<u8> = Vec::new();
        let verdef_size = ElfVerdef::size::<E>();
        let verdaux_size = ElfVerdaux::size::<E>();
        let mut prev: Option<usize> = None;
        let mut count = 0u32;

        let mut write = |contents: &mut Vec<u8>,
                         dynstr: &mut super::super::symtab::DynstrSection,
                         verstr: &[u8],
                         idx: u16,
                         flags: u16| {
            count += 1;
            if let Some(p) = prev {
                let mut vd = ElfVerdef::parse::<E>(&contents[p..]);
                vd.vd_next = (contents.len() - p) as u32;
                vd.write::<E>(&mut contents[p..]);
            }
            let pos = contents.len();
            prev = Some(pos);
            contents.resize(pos + verdef_size + verdaux_size, 0);
            ElfVerdef {
                vd_version: 1,
                vd_flags: flags,
                vd_ndx: idx,
                vd_cnt: 1,
                vd_hash: elf_hash(verstr),
                vd_aux: verdef_size as u32,
                vd_next: 0,
            }
            .write::<E>(&mut contents[pos..]);
            ElfVerdaux {
                vda_name: dynstr.add_string(verstr) as u32,
                vda_next: 0,
            }
            .write::<E>(&mut contents[pos + verdef_size..]);
        };

        let soname = if ctx.args.soname.is_empty() {
            path_filename(&ctx.args.output)
        } else {
            ctx.args.soname.clone()
        };
        write(
            &mut contents,
            &mut ctx.dynstr,
            soname.as_bytes(),
            1,
            VER_FLG_BASE as u16,
        );

        let defs = ctx.args.version_definitions.clone();
        for (i, verstr) in defs.iter().enumerate() {
            write(
                &mut contents,
                &mut ctx.dynstr,
                verstr.as_bytes(),
                VER_NDX_LAST_RESERVED as u16 + 1 + i as u16,
                0,
            );
        }

        let verdef = ctx.verdef.as_mut().unwrap();
        verdef.hdr.shdr.sh_info = count;
        verdef.contents = contents;
    }

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let shndx = ctx.dynstr.hdr.shndx;
        let verdef = ctx.verdef.as_mut().unwrap();
        verdef.hdr.shdr.sh_size = verdef.contents.len() as u64;
        verdef.hdr.shdr.sh_link = shndx;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let verdef = ctx.verdef.as_ref().unwrap();
        buf[..verdef.contents.len()].copy_from_slice(&verdef.contents);
    }
}
