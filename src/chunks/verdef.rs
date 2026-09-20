//! `.gnu.version_d`, defined symbol versions.

use rayon::prelude::*;

use crate::chunks::ChunkHeader;
use crate::chunks::dynstr::DynstrSection;
use crate::chunks::hash::elf_hash;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::target::Target;

// .gnu.version contains a parallel table for .dynsym to specify symbol
// versions of defined symbols. This section appears only in .so files,
// and it specifies the symbol version for each defined dynamic symbol.
#[derive(Debug)]
pub struct VerdefSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub contents: Vec<u8>,
}

impl<E: Layout> VerdefSection<E> {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::<E>::new(".gnu.version_d", SHT_GNU_VERDEF, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(4);
        Self { hdr, contents: Vec::new() }
    }
}

impl<E: Layout> Default for VerdefSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn construct<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("fill_verdef");
    if ctx.args.version_definitions.is_empty() {
        return;
    }

    // Handle --default-symver
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

    // Resize .gnu.version and write to it
    let n = ctx.dynsym.symbols.len();
    ctx.versym.contents.resize(n, VER_NDX_GLOBAL as u16);
    ctx.versym.contents[0] = VER_NDX_LOCAL as u16;

    ctx.versym.contents.par_iter_mut().zip(&ctx.dynsym.symbols).for_each(|(ver, &id)| {
        let Some(id) = id else { return };
        let sym = &ctx.symbols[id];
        if !matches!(sym.file(), Some(FileId::Obj(_))) {
            return;
        }
        // An unversioned undefined symbol takes version index 0.
        if sym.ver_idx as u32 != VER_NDX_UNSPECIFIED {
            *ver = sym.ver_idx;
        } else if sym.is_undef() {
            *ver = VER_NDX_LOCAL as u16;
        }
    });

    // Allocate a buffer for .gnu.version_d and write to it
    let verdef_size = ElfVerdef::<E>::size();
    let verdaux_size = ElfVerdaux::<E>::size();
    let mut contents: Vec<u8> =
        Vec::with_capacity((verdef_size + verdaux_size) * (ctx.args.version_definitions.len() + 1));
    let mut prev: Option<usize> = None;
    let mut count = 0u32;

    let mut write = |contents: &mut Vec<u8>,
                     dynstr: &mut DynstrSection<E>,
                     verstr: &[u8],
                     idx: u16,
                     flags: u16| {
        count += 1;
        if let Some(p) = prev {
            let mut vd = ElfVerdef::<E>::parse(&contents[p..]);
            vd.vd_next.set((contents.len() - p) as u32);
            vd.write(&mut contents[p..]);
        }
        let pos = contents.len();
        prev = Some(pos);
        contents.resize(pos + verdef_size + verdaux_size, 0);
        ElfVerdef::<E> {
            vd_version: U16::new(1),
            vd_flags: U16::new(flags),
            vd_ndx: U16::new(idx),
            vd_cnt: U16::new(1),
            vd_hash: U32::new(elf_hash(verstr)),
            vd_aux: U32::new(verdef_size as u32),
            vd_next: U32::default(),
        }
        .write(&mut contents[pos..]);
        ElfVerdaux::<E> {
            vda_name: U32::new(dynstr.add_string(verstr) as u32),
            vda_next: U32::default(),
        }
        .write(&mut contents[pos + verdef_size..]);
    };

    let soname = if ctx.args.soname.is_empty() {
        ctx.args.output.file_name().unwrap_or_default().as_encoded_bytes()
    } else {
        ctx.args.soname.as_encoded_bytes()
    };
    write(&mut contents, &mut ctx.dynstr, soname, 1, VER_FLG_BASE as u16);

    for (i, verstr) in ctx.args.version_definitions.iter().enumerate() {
        write(
            &mut contents,
            &mut ctx.dynstr,
            verstr,
            VER_NDX_LAST_RESERVED as u16 + 1 + i as u16,
            0,
        );
    }

    let verdef = ctx.verdef.as_mut().unwrap();
    verdef.hdr.shdr.sh_info.set(count);
    verdef.contents = contents;
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    let shndx = ctx.dynstr.hdr.shndx;
    let verdef = ctx.verdef.as_mut().unwrap();
    verdef.hdr.shdr.sh_size.set(verdef.contents.len() as u64);
    verdef.hdr.shdr.sh_link.set(shndx);
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let verdef = ctx.verdef.as_ref().unwrap();
    buf[..verdef.contents.len()].copy_from_slice(&verdef.contents);
}
