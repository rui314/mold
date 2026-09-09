//! Input file classification.

use crate::arch;
use crate::archive_file;
use crate::elf::*;
use crate::mapped_file::MappedFile;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Unknown,
    Empty,
    ElfObj,
    ElfDso,
    Ar,
    ThinAr,
    Text,
    GccLtoObj,
    LlvmBitcode,
}

fn is_text_file(data: &[u8]) -> bool {
    let is_text = |c: u8| c.is_ascii_graphic() || c == b' ' || c == b'\n' || c == b'\t';
    data.len() >= 4 && data[..4].iter().all(|&c| is_text(c))
}

/// Whether an ELF relocatable object is really a GCC LTO object.
fn is_gcc_lto_obj<E: Layout>(data: &[u8], has_gcc_plugin: bool) -> bool {
    let Some(ehdr) = data
        .get(..std::mem::size_of::<ElfEhdr<E>>())
        .map(record_from_bytes::<ElfEhdr<E>>)
    else {
        return false;
    };
    let Ok(shoff) = usize::try_from(ehdr.e_shoff.get()) else {
        return false;
    };
    if shoff == 0 {
        return false;
    }
    let shdr_size = std::mem::size_of::<ElfShdr<E>>();
    let Some(first_end) = shoff.checked_add(shdr_size) else {
        return false;
    };
    let Some(first_bytes) = data.get(shoff..first_end) else {
        return false;
    };
    let first = record_from_bytes::<ElfShdr<E>>(first_bytes);

    // e_shnum is a 16-bit field. If an object file contains more than 65279
    // sections, e_shnum is zero and the actual number is stored to the first
    // section header's sh_size field.
    let num_sections = if ehdr.e_shnum.get() == 0 {
        let Ok(num_sections) = usize::try_from(first.sh_size.get()) else {
            return false;
        };
        num_sections
    } else {
        ehdr.e_shnum.get() as usize
    };
    let Some(shdr_bytes_size) = num_sections.checked_mul(shdr_size) else {
        return false;
    };
    let Some(shdr_end) = shoff.checked_add(shdr_bytes_size) else {
        return false;
    };
    let Some(shdr_bytes) = data.get(shoff..shdr_end) else {
        return false;
    };
    let shdrs = records_from_bytes::<ElfShdr<E>>(shdr_bytes);

    // e_shstrndx is a 16-bit field. If .shstrtab's section index is
    // too large, the actual number is stored to sh_link field.
    let shstrtab_idx = if u32::from(ehdr.e_shstrndx.get()) == SHN_XINDEX {
        first.sh_link.get() as usize
    } else {
        ehdr.e_shstrndx.get() as usize
    };
    let shstrtab_offset = if has_gcc_plugin {
        shdrs
            .get(shstrtab_idx)
            .map(|shdr| shdr.sh_offset.get() as usize)
    } else {
        None
    };

    for i in 0..shdrs.len() {
        // GCC FAT LTO objects contain both regular ELF sections and GCC-
        // specific LTO sections, so that they can be linked as LTO objects if
        // the LTO linker plugin is available and falls back as regular
        // objects otherwise. GCC FAT LTO object can be identified by the
        // presence of `.gcc.lto_.symtab` section.
        if let Some(offset) = shstrtab_offset {
            let name = crate::util::cstr_at(data, offset + shdrs[i].sh_name.get() as usize);
            if name.starts_with(b".gnu.lto_.symtab.") {
                return true;
            }
        }

        if shdrs[i].sh_type.get() != SHT_SYMTAB {
            continue;
        }
        let shdr = &shdrs[i];

        // GCC non-FAT LTO object contains only sections symbols followed by
        // a common symbol whose name is `__gnu_lto_slim` (or `__gnu_lto_v1`
        // for older GCC releases).
        let off = shdr.sh_offset.get() as usize;
        let Some(bytes) = data.get(off..off + shdr.sh_size.get() as usize) else {
            return false;
        };
        if !bytes.len().is_multiple_of(std::mem::size_of::<ElfSym<E>>()) {
            return false;
        }
        let syms = records_from_bytes::<ElfSym<E>>(bytes).iter();
        let skip = |ty: u32| ty == STT_NOTYPE || ty == STT_FILE || ty == STT_SECTION;

        if let Some(sym) = syms.skip(1).find(|s| !skip(s.st_type())) {
            if sym.st_shndx().get() as u32 == SHN_COMMON {
                let Some(strtab) = shdrs.get(shdr.sh_link.get() as usize) else {
                    return false;
                };
                let name = crate::util::cstr_at(
                    data,
                    strtab.sh_offset.get() as usize + sym.st_name().get() as usize,
                );
                if name.starts_with(b"__gnu_lto_") {
                    return true;
                }
            }
        }
        break;
    }
    false
}

/// Classifies a file by its contents.
pub fn get_file_type(plugin: &std::path::Path, mf: &MappedFile) -> FileType {
    let data = mf.data();
    if data.is_empty() {
        return FileType::Empty;
    }

    // GCC FAT LTO objects can be linked as regular ELF objects. If the active
    // plugin is LLVM's, treat them as regular objects so that we can fall back
    // to native code instead of routing them through GCC LTO handling.
    let has_gcc_plugin = !plugin.as_os_str().is_empty()
        && !plugin
            .as_os_str()
            .as_encoded_bytes()
            .windows(9)
            .any(|s| s == b"LLVMgold.");

    if data.starts_with(b"\x7fELF") && data.len() >= 20 {
        let is_le = data[EI_DATA as usize] == ELFDATA2LSB as u8;
        let is_32 = data[EI_CLASS as usize] == ELFCLASS32 as u8;
        let e_type = if is_le {
            u16::from_le_bytes([data[16], data[17]]) as u32
        } else {
            u16::from_be_bytes([data[16], data[17]]) as u32
        };

        if e_type == ET_REL {
            let is_lto = match (is_le, is_32) {
                (true, true) => is_gcc_lto_obj::<arch::I386>(data, has_gcc_plugin),
                (true, false) => is_gcc_lto_obj::<arch::X86_64>(data, has_gcc_plugin),
                (false, true) => is_gcc_lto_obj::<arch::M68k>(data, has_gcc_plugin),
                (false, false) => is_gcc_lto_obj::<arch::Sparc64>(data, has_gcc_plugin),
            };
            return if is_lto {
                FileType::GccLtoObj
            } else {
                FileType::ElfObj
            };
        }
        if e_type == ET_DYN {
            return FileType::ElfDso;
        }
        return FileType::Unknown;
    }

    if data.starts_with(b"!<arch>\n") {
        return FileType::Ar;
    }
    if data.starts_with(b"!<thin>\n") {
        return FileType::ThinAr;
    }
    if is_text_file(data) {
        return FileType::Text;
    }
    if data.starts_with(b"\xde\xc0\x17\x0b") || data.starts_with(b"BC\xc0\xde") {
        return FileType::LlvmBitcode;
    }
    FileType::Unknown
}

/// Returns the target name of an ELF file, or `None` if its machine type
/// is not one we recognize.
pub fn get_elf_target(data: &[u8]) -> Option<&'static str> {
    if data.len() < 52 {
        return None;
    }
    let is_le = data[EI_DATA as usize] == ELFDATA2LSB as u8;
    let is_64 = data[EI_CLASS as usize] == ELFCLASS64 as u8;
    let read_u16 = |off: usize| {
        let b = [data[off], data[off + 1]];
        if is_le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    };
    let read_u32 = |off: usize| {
        let b = [data[off], data[off + 1], data[off + 2], data[off + 3]];
        if is_le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    let e_machine = read_u16(18) as u32;
    // e_flags follows the three word-sized fields after e_version.
    let e_flags = read_u32(if is_64 { 48 } else { 36 });

    let name = match e_machine {
        EM_386 => "i386",
        EM_X86_64 => "x86_64",
        EM_ARM => {
            if is_le {
                "arm32"
            } else {
                "arm32be"
            }
        }
        EM_AARCH64 => {
            if is_le {
                "arm64"
            } else {
                "arm64be"
            }
        }
        EM_RISCV => match (is_le, is_64) {
            (true, true) => "riscv64",
            (true, false) => "riscv32",
            (false, true) => "riscv64be",
            (false, false) => "riscv32be",
        },
        EM_PPC => "ppc32",
        EM_PPC64 => {
            // ELFv1 is big-endian and ELFv2 is little-endian by convention, but
            // the correspondence is not a rule; musl for example uses ELFv2 on
            // big-endian too. We support only the usual combinations, so treat
            // the others as unrecognizable rather than silently linking them
            // against the wrong ABI.
            let abi = e_flags & EF_PPC64_ABI;
            if !is_le && (abi == 0 || abi == 1) {
                "ppc64v1"
            } else if is_le && (abi == 0 || abi == 2) {
                "ppc64v2"
            } else {
                return None;
            }
        }
        EM_S390X => "s390x",
        EM_SPARC64 => "sparc64",
        EM_68K => "m68k",
        EM_SH => {
            if is_le {
                "sh4"
            } else {
                "sh4be"
            }
        }
        EM_LOONGARCH => {
            if is_64 {
                "loongarch64"
            } else {
                "loongarch32"
            }
        }
        _ => return None,
    };
    Some(name)
}

// Read the beginning of a given file and returns its machine type
// (e.g. EM_X86_64 or EM_386).
pub fn get_machine_type(
    plugin: &std::path::Path,
    mf: &'static MappedFile,
    script_target: impl FnOnce() -> Option<&'static str>,
) -> Option<&'static str> {
    match get_file_type(plugin, mf) {
        FileType::ElfObj | FileType::ElfDso | FileType::GccLtoObj => get_elf_target(mf.data()),
        FileType::Ar => archive_file::read_fat_archive_members(mf)
            .into_iter()
            .find(|child| {
                matches!(
                    get_file_type(plugin, child),
                    FileType::ElfObj | FileType::GccLtoObj
                )
            })
            .and_then(|child| get_elf_target(child.data())),
        FileType::ThinAr => archive_file::read_thin_archive_members(mf)
            .into_iter()
            .find(|child| {
                matches!(
                    get_file_type(plugin, child),
                    FileType::ElfObj | FileType::GccLtoObj
                )
            })
            .and_then(|child| get_elf_target(child.data())),
        FileType::Text => script_target(),
        _ => None,
    }
}
