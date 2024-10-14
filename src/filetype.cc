#include "mold.h"
#include "../lib/archive-file.h"

namespace mold {

static bool is_text_file(MappedFile *mf) {
  auto istext = [](char c) {
    return isprint(c) || c == '\n' || c == '\t';
  };

  u8 *data = mf->data;
  return mf->size >= 4 && istext(data[0]) && istext(data[1]) &&
         istext(data[2]) && istext(data[3]);
}

template <typename E>
static bool is_gcc_lto_obj(MappedFile *mf, bool has_plugin) {
  const char *data = mf->get_contents().data();
  ElfEhdr<E> &ehdr = *(ElfEhdr<E> *)data;
  ElfShdr<E> *sh_begin = (ElfShdr<E> *)(data + ehdr.e_shoff);
  std::span<ElfShdr<E>> shdrs{(ElfShdr<E> *)(data + ehdr.e_shoff), ehdr.e_shnum};

  // e_shstrndx is a 16-bit field. If .shstrtab's section index is
  // too large, the actual number is stored to sh_link field.
  i64 shstrtab_idx = (ehdr.e_shstrndx == SHN_XINDEX)
    ? sh_begin->sh_link : ehdr.e_shstrndx;

  for (ElfShdr<E> &sec : shdrs) {
    // GCC FAT LTO objects contain both regular ELF sections and GCC-
    // specific LTO sections, so that they can be linked as LTO objects if
    // the LTO linker plugin is available and falls back as regular
    // objects otherwise. GCC FAT LTO object can be identified by the
    // presence of `.gcc.lto_.symtab` section.
    if (has_plugin) {
      std::string_view name = data + shdrs[shstrtab_idx].sh_offset + sec.sh_name;
      if (name.starts_with(".gnu.lto_.symtab."))
        return true;
    }

    if (sec.sh_type != SHT_SYMTAB)
      continue;

    // GCC non-FAT LTO object contains only sections symbols followed by
    // a common symbol whose name is `__gnu_lto_slim` (or `__gnu_lto_v1`
    // for older GCC releases).
    std::span<ElfSym<E>> elf_syms{(ElfSym<E> *)(data + sec.sh_offset),
                                  (size_t)sec.sh_size / sizeof(ElfSym<E>)};

    auto skip = [](u8 type) {
      return type == STT_NOTYPE || type == STT_FILE || type == STT_SECTION;
    };

    i64 i = 1;
    while (i < elf_syms.size() && skip(elf_syms[i].st_type))
      i++;

    if (i < elf_syms.size() && elf_syms[i].st_shndx == SHN_COMMON) {
      std::string_view name =
        data + shdrs[sec.sh_link].sh_offset + elf_syms[i].st_name;
      if (name.starts_with("__gnu_lto_"))
        return true;
    }
    break;
  }

  return false;
}

template <typename E>
FileType get_file_type(Context<E> &ctx, MappedFile *mf) {
  std::string_view data = mf->get_contents();
  bool has_plugin = !ctx.arg.plugin.empty();

  if (data.empty())
    return FileType::EMPTY;

  if (data.starts_with("\177ELF")) {
    u8 byte_order = ((ElfEhdr<I386> *)data.data())->e_ident[EI_DATA];

    if (byte_order == ELFDATA2LSB) {
      auto &ehdr = *(ElfEhdr<I386> *)data.data();

      if (ehdr.e_type == ET_REL) {
        if (ehdr.e_ident[EI_CLASS] == ELFCLASS32) {
          if (is_gcc_lto_obj<I386>(mf, has_plugin))
            return FileType::GCC_LTO_OBJ;
        } else {
          if (is_gcc_lto_obj<X86_64>(mf, has_plugin))
            return FileType::GCC_LTO_OBJ;
        }
        return FileType::ELF_OBJ;
      }

      if (ehdr.e_type == ET_DYN)
        return FileType::ELF_DSO;
    } else {
      auto &ehdr = *(ElfEhdr<M68K> *)data.data();

      if (ehdr.e_type == ET_REL) {
        if (ehdr.e_ident[EI_CLASS] == ELFCLASS32) {
          if (is_gcc_lto_obj<M68K>(mf, has_plugin))
            return FileType::GCC_LTO_OBJ;
        } else {
          if (is_gcc_lto_obj<SPARC64>(mf, has_plugin))
            return FileType::GCC_LTO_OBJ;
        }
        return FileType::ELF_OBJ;
      }

      if (ehdr.e_type == ET_DYN)
        return FileType::ELF_DSO;
    }
    return FileType::UNKNOWN;
  }

  if (data.starts_with("!<arch>\n"))
    return FileType::AR;
  if (data.starts_with("!<thin>\n"))
    return FileType::THIN_AR;
  if (is_text_file(mf))
    return FileType::TEXT;
  if (data.starts_with("\xde\xc0\x17\x0b"))
    return FileType::LLVM_BITCODE;
  if (data.starts_with("BC\xc0\xde"))
    return FileType::LLVM_BITCODE;
  return FileType::UNKNOWN;
}

static std::string_view get_elf_type(u8 *buf) {
  bool is_le = (buf[EI_DATA] == ELFDATA2LSB);
  bool is_64 = (buf[EI_CLASS] == ELFCLASS64);

  auto *ehdr_le = (ElfEhdr<I386> *)buf;
  auto *ehdr_be = (ElfEhdr<M68K> *)buf;

  switch (is_le ? ehdr_le->e_machine : ehdr_be->e_machine) {
  case EM_386:
    return I386::name;
  case EM_X86_64:
    return X86_64::name;
  case EM_ARM:
    return ARM32::name;
  case EM_AARCH64:
    return ARM64::name;
  case EM_RISCV:
    if (is_le)
      return is_64 ? RV64LE::name : RV32LE::name;
    return is_64 ? RV64BE::name : RV32BE::name;
  case EM_PPC:
    return PPC32::name;
  case EM_PPC64:
    return is_le ? PPC64V2::name : PPC64V1::name;
  case EM_S390X:
    return S390X::name;
  case EM_SPARC64:
    return SPARC64::name;
  case EM_68K:
    return M68K::name;
  case EM_SH:
    return is_le ? SH4LE::name : SH4BE::name;
  case EM_LOONGARCH:
    return is_64 ? LOONGARCH64::name : LOONGARCH32::name;
  default:
    return "";
  }
}

// Read the beginning of a given file and returns its machine type
// (e.g. EM_X86_64 or EM_386).
template <typename E>
std::string_view
get_machine_type(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf) {
  switch (get_file_type(ctx, mf)) {
  case FileType::ELF_OBJ:
  case FileType::ELF_DSO:
  case FileType::GCC_LTO_OBJ:
    return get_elf_type(mf->data);
  case FileType::AR:
    for (MappedFile *child : read_fat_archive_members(ctx, mf))
      if (FileType ty = get_file_type(ctx, child);
          ty == FileType::ELF_OBJ || ty == FileType::GCC_LTO_OBJ)
        return get_elf_type(child->data);
    return "";
  case FileType::THIN_AR:
    for (MappedFile *child : read_thin_archive_members(ctx, mf))
      if (FileType ty = get_file_type(ctx, child);
          ty == FileType::ELF_OBJ || ty == FileType::GCC_LTO_OBJ)
        return get_elf_type(child->data);
    return "";
  case FileType::TEXT:
    return Script(ctx, rctx, mf).get_script_output_type();
  default:
    return "";
  }
}

using E = MOLD_TARGET;

template FileType get_file_type(Context<E> &, MappedFile *);

template std::string_view
get_machine_type(Context<E> &, ReaderContext &, MappedFile *);

} // namespace mold
