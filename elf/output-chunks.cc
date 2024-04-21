#include "mold.h"

#include "config.h"
#include "blake3.h"

#include <cctype>
#include <set>
#include <shared_mutex>
#include <span>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_scan.h>
#include <tbb/parallel_sort.h>

namespace mold::elf {

// The hash function for .hash.
static u32 elf_hash(std::string_view name) {
  u32 h = 0;
  for (u8 c : name) {
    h = (h << 4) + c;
    u32 g = h & 0xf0000000;
    if (g != 0)
      h ^= g >> 24;
    h &= ~g;
  }
  return h;
}

// The hash function for .gnu.hash.
static u32 djb_hash(std::string_view name) {
  u32 h = 5381;
  for (u8 c : name)
    h = (h << 5) + h + c;
  return h;
}

template <typename E>
static u64 get_entry_addr(Context<E> &ctx) {
  if (ctx.arg.relocatable)
    return 0;

  if (Symbol<E> &sym = *ctx.arg.entry;
      sym.file && !sym.file->is_dso)
    return sym.get_addr(ctx);

  if (OutputSection<E> *osec = find_section(ctx, ".text"))
    return osec->shdr.sh_addr;
  return 0;
}

template <typename E>
void OutputEhdr<E>::copy_buf(Context<E> &ctx) {
  ElfEhdr<E> &hdr = *(ElfEhdr<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(&hdr, 0, sizeof(hdr));

  memcpy(&hdr.e_ident, "\177ELF", 4);
  hdr.e_ident[EI_CLASS] = E::is_64 ? ELFCLASS64 : ELFCLASS32;
  hdr.e_ident[EI_DATA] = E::is_le ? ELFDATA2LSB : ELFDATA2MSB;
  hdr.e_ident[EI_VERSION] = EV_CURRENT;
  hdr.e_machine = E::e_machine;
  hdr.e_version = EV_CURRENT;
  hdr.e_entry = get_entry_addr(ctx);
  hdr.e_flags = get_eflags(ctx);
  hdr.e_ehsize = sizeof(ElfEhdr<E>);

  // If e_shstrndx is too large, a dummy value is set to e_shstrndx.
  // The real value is stored to the zero'th section's sh_link field.
  if (ctx.shstrtab) {
    if (ctx.shstrtab->shndx < SHN_LORESERVE)
      hdr.e_shstrndx = ctx.shstrtab->shndx;
    else
      hdr.e_shstrndx = SHN_XINDEX;
  }

  if (ctx.arg.relocatable)
    hdr.e_type = ET_REL;
  else if (ctx.arg.pic)
    hdr.e_type = ET_DYN;
  else
    hdr.e_type = ET_EXEC;

  if (ctx.phdr) {
    hdr.e_phoff = ctx.phdr->shdr.sh_offset;
    hdr.e_phentsize = sizeof(ElfPhdr<E>);
    hdr.e_phnum = ctx.phdr->shdr.sh_size / sizeof(ElfPhdr<E>);
  }

  if (ctx.shdr) {
    hdr.e_shoff = ctx.shdr->shdr.sh_offset;
    hdr.e_shentsize = sizeof(ElfShdr<E>);

    // Since e_shnum is a 16-bit integer field, we can't store a very
    // large value there. If it is >65535, the real value is stored to
    // the zero'th section's sh_size field.
    i64 shnum = ctx.shdr->shdr.sh_size / sizeof(ElfShdr<E>);
    hdr.e_shnum = (shnum <= UINT16_MAX) ? shnum : 0;
  }
}

template <typename E>
void OutputShdr<E>::copy_buf(Context<E> &ctx) {
  ElfShdr<E> *hdr = (ElfShdr<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(hdr, 0, this->shdr.sh_size);

  i64 shnum = ctx.shdr->shdr.sh_size / sizeof(ElfShdr<E>);
  if (UINT16_MAX < shnum)
    hdr->sh_size = shnum;

  if (ctx.shstrtab && SHN_LORESERVE <= ctx.shstrtab->shndx)
    hdr->sh_link = ctx.shstrtab->shndx;

  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->shndx)
      hdr[chunk->shndx] = chunk->shdr;
}

template <typename E>
i64 to_phdr_flags(Context<E> &ctx, Chunk<E> *chunk) {
  // All sections are put into a single RWX segment if --omagic
  if (ctx.arg.omagic)
    return PF_R | PF_W | PF_X;

  bool write = (chunk->shdr.sh_flags & SHF_WRITE);
  bool exec = (chunk->shdr.sh_flags & SHF_EXECINSTR);

  // .text is not readable if --execute-only
  if (exec && ctx.arg.execute_only) {
    if (write)
      Error(ctx) << "--execute-only is not compatible with writable section: "
                 << chunk->name;
    return PF_X;
  }

  // .rodata is merged with .text if --no-rosegment
  if (!write && !ctx.arg.rosegment)
    exec = true;

  return PF_R | (write ? PF_W : PF_NONE) | (exec ? PF_X : PF_NONE);
}

template <typename E>
static std::vector<ElfPhdr<E>> create_phdr(Context<E> &ctx) {
  std::vector<ElfPhdr<E>> vec;

  auto define = [&](u64 type, u64 flags, Chunk<E> *chunk) {
    ElfPhdr<E> phdr = {};
    phdr.p_type = type;
    phdr.p_flags = flags;
    phdr.p_align = chunk->shdr.sh_addralign;
    phdr.p_offset = chunk->shdr.sh_offset;

    if (chunk->shdr.sh_type != SHT_NOBITS)
      phdr.p_filesz = chunk->shdr.sh_size;

    phdr.p_vaddr = chunk->shdr.sh_addr;
    phdr.p_paddr = chunk->shdr.sh_addr;

    if (chunk->shdr.sh_flags & SHF_ALLOC)
      phdr.p_memsz = chunk->shdr.sh_size;
    vec.push_back(phdr);
  };

  auto append = [&](Chunk<E> *chunk) {
    ElfPhdr<E> &phdr = vec.back();
    phdr.p_align = std::max<u64>(phdr.p_align, chunk->shdr.sh_addralign);
    phdr.p_memsz = chunk->shdr.sh_addr + chunk->shdr.sh_size - phdr.p_vaddr;
    if (chunk->shdr.sh_type != SHT_NOBITS)
      phdr.p_filesz = phdr.p_memsz;
  };

  auto is_bss = [](Chunk<E> *chunk) {
    return chunk->shdr.sh_type == SHT_NOBITS;
  };

  auto is_tbss = [](Chunk<E> *chunk) {
    return chunk->shdr.sh_type == SHT_NOBITS &&
           (chunk->shdr.sh_flags & SHF_TLS);
  };

  auto is_note = [](Chunk<E> *chunk) {
    return chunk->shdr.sh_type == SHT_NOTE;
  };

  // When we are creating PT_LOAD segments, we consider only
  // the following chunks.
  std::vector<Chunk<E> *> chunks;
  for (Chunk<E> *chunk : ctx.chunks)
    if ((chunk->shdr.sh_flags & SHF_ALLOC) && !is_tbss(chunk))
      chunks.push_back(chunk);

  // The ELF spec says that "loadable segment entries in the program
  // header table appear in ascending order, sorted on the p_vaddr
  // member".
  sort(chunks, [](Chunk<E> *a, Chunk<E> *b) {
    return a->shdr.sh_addr < b->shdr.sh_addr;
  });

  // Create a PT_PHDR for the program header itself.
  if (ctx.phdr && (ctx.phdr->shdr.sh_flags & SHF_ALLOC))
    define(PT_PHDR, PF_R, ctx.phdr);

  // Create a PT_INTERP.
  if (ctx.interp)
    define(PT_INTERP, PF_R, ctx.interp);

  // Create a PT_NOTE for SHF_NOTE sections.
  for (i64 i = 0; i < chunks.size();) {
    Chunk<E> *first = chunks[i++];
    if (is_note(first)) {
      i64 flags = to_phdr_flags(ctx, first);
      define(PT_NOTE, flags, first);

      while (i < chunks.size() &&
             is_note(ctx.chunks[i]) &&
             to_phdr_flags(ctx, ctx.chunks[i]) == flags)
        append(ctx.chunks[i++]);
    }
  }

  // Create PT_LOAD segments.
  for (i64 i = 0; i < chunks.size();) {
    Chunk<E> *first = chunks[i++];
    i64 flags = to_phdr_flags(ctx, first);
    define(PT_LOAD, flags, first);
    vec.back().p_align = std::max<u64>(ctx.page_size, vec.back().p_align);

    // Add contiguous ALLOC sections as long as they have the same
    // section flags and there's no on-disk gap in between.
    if (!is_bss(first))
      while (i < chunks.size() &&
             !is_bss(chunks[i]) &&
             to_phdr_flags(ctx, chunks[i]) == flags &&
             chunks[i]->shdr.sh_offset - first->shdr.sh_offset ==
             chunks[i]->shdr.sh_addr - first->shdr.sh_addr)
        append(chunks[i++]);

    while (i < chunks.size() &&
           is_bss(chunks[i]) &&
           to_phdr_flags(ctx, chunks[i]) == flags)
      append(chunks[i++]);
  }

  // Create a PT_TLS.
  for (i64 i = 0; i < ctx.chunks.size();) {
    Chunk<E> *first = ctx.chunks[i++];
    if (first->shdr.sh_flags & SHF_TLS) {
      define(PT_TLS, PF_R, first);
      while (i < ctx.chunks.size() &&
             (ctx.chunks[i]->shdr.sh_flags & SHF_TLS))
        append(ctx.chunks[i++]);
    }
  }

  // Add PT_DYNAMIC
  if (ctx.dynamic && ctx.dynamic->shdr.sh_size)
    define(PT_DYNAMIC, PF_R | PF_W, ctx.dynamic);

  // Add PT_GNU_EH_FRAME
  if (ctx.eh_frame_hdr)
    define(PT_GNU_EH_FRAME, PF_R, ctx.eh_frame_hdr);

  // Add PT_GNU_STACK, which is a marker segment that doesn't really
  // contain any segments. It controls executable bit of stack area.
  {
    ElfPhdr<E> phdr = {};
    phdr.p_type = PT_GNU_STACK;
    phdr.p_flags = ctx.arg.z_execstack ? (PF_R | PF_W | PF_X) : (PF_R | PF_W);
    phdr.p_memsz = ctx.arg.z_stack_size;
    phdr.p_align = 1;
    vec.push_back(phdr);
  }

  // Create a PT_GNU_RELRO.
  if (ctx.arg.z_relro) {
    for (i64 i = 0; i < chunks.size();) {
      Chunk<E> *first = chunks[i++];
      if (first->is_relro) {
        define(PT_GNU_RELRO, PF_R, first);
        while (i < chunks.size() && chunks[i]->is_relro)
          append(chunks[i++]);
        vec.back().p_align = 1;
      }
    }
  }

  // Create a PT_ARM_EDXIDX
  if constexpr (is_arm32<E>)
    if (OutputSection<E> *osec = find_section(ctx, SHT_ARM_EXIDX))
      define(PT_ARM_EXIDX, PF_R, osec);

  // Create a PT_RISCV_ATTRIBUTES
  if constexpr (is_riscv<E>)
    if (ctx.extra.riscv_attributes->shdr.sh_size)
      define(PT_RISCV_ATTRIBUTES, PF_R, ctx.extra.riscv_attributes);

  // Create a PT_OPENBSD_RANDOMIZE
  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->name == ".openbsd.randomdata")
      define(PT_OPENBSD_RANDOMIZE, PF_R | PF_W, chunk);

  // Set p_paddr if --physical-image-base was given. --physical-image-base
  // is typically used in embedded programming to specify the base address
  // of a memory-mapped ROM area. In that environment, paddr refers to a
  // segment's initial location in ROM and vaddr refers the its run-time
  // address.
  //
  // When a device is turned on, it start executing code at a fixed
  // location in the ROM area. At that location is a startup routine that
  // copies data or code from ROM to RAM before using them.
  //
  // .data must have different paddr and vaddr because ROM is not writable.
  // paddr of .rodata and .text may or may be equal to vaddr. They can be
  // directly read or executed from ROM, but oftentimes they are copied
  // from ROM to RAM because Flash or EEPROM are usually much slower than
  // DRAM.
  //
  // We want to keep vaddr == pvaddr for as many segments as possible so
  // that they can be directly read/executed from ROM. If a gap between
  // two segments is two page size or larger, we give up and pack segments
  // tightly so that we don't waste too much ROM area.
  if (ctx.arg.physical_image_base) {
    for (i64 i = 0; i < vec.size(); i++) {
      if (vec[i].p_type != PT_LOAD)
        continue;

      u64 addr = *ctx.arg.physical_image_base;
      bool in_sync = (vec[i].p_vaddr == addr);

      vec[i].p_paddr = addr;
      addr += vec[i].p_memsz;

      for (i++; i < vec.size() && vec[i].p_type == PT_LOAD; i++) {
        ElfPhdr<E> &p = vec[i];
        if (in_sync && addr <= p.p_vaddr && p.p_vaddr < addr + ctx.page_size * 2) {
          p.p_paddr = p.p_vaddr;
          addr = p.p_vaddr + p.p_memsz;
        } else {
          in_sync = false;
          p.p_paddr = addr;
          addr += p.p_memsz;
        }
      }
      break;
    }
  }

  vec.resize(vec.size() + ctx.arg.spare_program_headers);
  return vec;
}

template <typename E>
void OutputPhdr<E>::update_shdr(Context<E> &ctx) {
  phdrs = create_phdr(ctx);
  this->shdr.sh_size = phdrs.size() * sizeof(ElfPhdr<E>);

  ctx.tls_begin = get_tls_begin(ctx);
  ctx.tp_addr = get_tp_addr(ctx);
  ctx.dtp_addr = get_dtp_addr(ctx);
}

template <typename E>
void OutputPhdr<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->shdr.sh_offset, phdrs);
}

template <typename E>
void InterpSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = ctx.arg.dynamic_linker.size() + 1;
}

template <typename E>
void InterpSection<E>::copy_buf(Context<E> &ctx) {
  write_string(ctx.buf + this->shdr.sh_offset, ctx.arg.dynamic_linker);
}

template <typename E>
void RelDynSection<E>::update_shdr(Context<E> &ctx) {
  i64 offset = 0;

  for (Chunk<E> *chunk : ctx.chunks) {
    chunk->reldyn_offset = offset;
    offset += chunk->get_reldyn_size(ctx) * sizeof(ElfRel<E>);
  }

  for (ObjectFile<E> *file : ctx.objs) {
    file->reldyn_offset = offset;
    offset += file->num_dynrel * sizeof(ElfRel<E>);
  }

  this->shdr.sh_size = offset;
  this->shdr.sh_link = ctx.dynsym->shndx;
}

template <typename E>
void RelDynSection<E>::sort(Context<E> &ctx) {
  Timer t(ctx, "sort_dynamic_relocs");

  ElfRel<E> *begin = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset);
  ElfRel<E> *end = begin + this->shdr.sh_size / sizeof(ElfRel<E>);

  auto get_rank = [](u32 r_type) {
    if (r_type == E::R_RELATIVE)
      return 0;
    if constexpr (supports_ifunc<E>)
      if (r_type == E::R_IRELATIVE)
        return 2;
    return 1;
  };

  // This is the reason why we sort dynamic relocations. Quote from
  // https://www.airs.com/blog/archives/186:
  //
  //   The dynamic linker in glibc uses a one element cache when processing
  //   relocs: if a relocation refers to the same symbol as the previous
  //   relocation, then the dynamic linker reuses the value rather than
  //   looking up the symbol again. Thus the dynamic linker gets the best
  //   results if the dynamic relocations are sorted so that all dynamic
  //   relocations for a given dynamic symbol are adjacent.
  //
  //   Other than that, the linker sorts together all relative relocations,
  //   which don't have symbols. Two relative relocations, or two relocations
  //   against the same symbol, are sorted by the address in the output
  //   file. This tends to optimize paging and caching when there are two
  //   references from the same page.
  //
  // We group IFUNC relocations at the end of .rel.dyn because we want to
  // apply all the other relocations before running user-supplied ifunc
  // resolver functions.
  tbb::parallel_sort(begin, end, [&](const ElfRel<E> &a, const ElfRel<E> &b) {
    return std::tuple(get_rank(a.r_type), a.r_sym, a.r_offset) <
           std::tuple(get_rank(b.r_type), b.r_sym, b.r_offset);
  });
}

template <typename E>
void RelrDynSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_link = ctx.dynsym->shndx;

  i64 n = 0;
  for (Chunk<E> *chunk : ctx.chunks)
    n += chunk->relr.size();
  this->shdr.sh_size = n * sizeof(Word<E>);
}

template <typename E>
void RelrDynSection<E>::copy_buf(Context<E> &ctx) {
  Word<E> *buf = (Word<E> *)(ctx.buf + this->shdr.sh_offset);

  for (Chunk<E> *chunk : ctx.chunks)
    for (u64 val : chunk->relr)
      *buf++ = (val & 1) ? val : (chunk->shdr.sh_addr + val);
}

template <typename E>
void StrtabSection<E>::update_shdr(Context<E> &ctx) {
  i64 offset = 1;

  // ARM32 uses $a, $t and $t mapping symbols to mark the beginning of
  // ARM, Thumb and data in text, respectively. These symbols don't
  // affect correctness of the program but helps disassembler to
  // disassemble machine code appropriately.
  if constexpr (is_arm32<E>)
    if (!ctx.arg.strip_all && !ctx.arg.retain_symbols_file)
      offset += sizeof("$a\0$t\0$d");

  for (Chunk<E> *chunk : ctx.chunks) {
    chunk->strtab_offset = offset;
    offset += chunk->strtab_size;
  }

  for (ObjectFile<E> *file : ctx.objs) {
    file->strtab_offset = offset;
    offset += file->strtab_size;
  }

  for (SharedFile<E> *file : ctx.dsos) {
    file->strtab_offset = offset;
    offset += file->strtab_size;
  }

  this->shdr.sh_size = (offset == 1) ? 0 : offset;
}

template <typename E>
void StrtabSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  buf[0] = '\0';

  if constexpr (is_arm32<E>)
    if (!ctx.arg.strip_all && !ctx.arg.retain_symbols_file)
      memcpy(buf + 1, "$a\0$t\0$d", 9);
}

template <typename E>
void ShstrtabSection<E>::update_shdr(Context<E> &ctx) {
  std::unordered_map<std::string_view, i64> map;
  i64 offset = 1;

  for (Chunk<E> *chunk : ctx.chunks) {
    if (!chunk->is_header() && !chunk->name.empty()) {
      auto [it, inserted] = map.insert({chunk->name, offset});
      chunk->shdr.sh_name = it->second;
      if (inserted)
        offset += chunk->name.size() + 1;
    }
  }

  this->shdr.sh_size = offset;
}

template <typename E>
void ShstrtabSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  base[0] = '\0';

  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->shdr.sh_name)
      write_string(base + chunk->shdr.sh_name, chunk->name);
}

template <typename E>
i64 DynstrSection<E>::add_string(std::string_view str) {
  if (this->shdr.sh_size == 0)
    this->shdr.sh_size = 1;

  if (str.empty())
    return 0;

  auto [it, inserted] = strings.insert({str, this->shdr.sh_size});
  if (inserted)
    this->shdr.sh_size += str.size() + 1;
  return it->second;
}

template <typename E>
i64 DynstrSection<E>::find_string(std::string_view str) {
  if (str.empty())
    return 0;

  auto it = strings.find(str);
  assert(it != strings.end());
  return it->second;
}

template <typename E>
void DynstrSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  base[0] = '\0';

  for (std::pair<std::string_view, i64> p : strings)
    write_string(base + p.second, p.first);

  i64 off = dynsym_offset;
  for (Symbol<E> *sym : ctx.dynsym->symbols)
    if (sym)
      off += write_string(base + off, sym->name());
}

template <typename E>
void SymtabSection<E>::update_shdr(Context<E> &ctx) {
  i64 nsyms = 1;

  // Section symbols
  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->shndx)
      nsyms++;

  // Linker-synthesized symbols
  for (Chunk<E> *chunk : ctx.chunks) {
    chunk->local_symtab_idx = nsyms;
    nsyms += chunk->num_local_symtab;
  }

  // File local symbols
  for (ObjectFile<E> *file : ctx.objs) {
    file->local_symtab_idx = nsyms;
    nsyms += file->num_local_symtab;
  }

  // File global symbols
  for (ObjectFile<E> *file : ctx.objs) {
    file->global_symtab_idx = nsyms;
    nsyms += file->num_global_symtab;
  }

  for (SharedFile<E> *file : ctx.dsos) {
    file->global_symtab_idx = nsyms;
    nsyms += file->num_global_symtab;
  }

  this->shdr.sh_info = ctx.objs[0]->global_symtab_idx;
  this->shdr.sh_link = ctx.strtab->shndx;
  this->shdr.sh_size = (nsyms == 1) ? 0 : nsyms * sizeof(ElfSym<E>);
}

template <typename E>
void SymtabSection<E>::copy_buf(Context<E> &ctx) {
  ElfSym<E> *buf = (ElfSym<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, sizeof(ElfSym<E>));

  if (ctx.symtab_shndx) {
    ElfShdr<E> &shdr = ctx.symtab_shndx->shdr;
    memset(ctx.buf + shdr.sh_offset, 0, shdr.sh_size);
  }

  // Create section symbols
  for (Chunk<E> *chunk : ctx.chunks) {
    if (chunk->shndx) {
      ElfSym<E> &sym = buf[chunk->shndx];
      memset(&sym, 0, sizeof(sym));
      sym.st_type = STT_SECTION;
      sym.st_value = chunk->shdr.sh_addr;

      if (ctx.symtab_shndx) {
        U32<E> *xindex = (U32<E> *)(ctx.buf + ctx.symtab_shndx->shdr.sh_offset);
        xindex[chunk->shndx] = chunk->shndx;
        sym.st_shndx = SHN_XINDEX;
      } else {
        sym.st_shndx = chunk->shndx;
      }
    }
  }

  // Populate linker-synthesized symbols
  tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
    chunk->populate_symtab(ctx);
  });

  // Copy symbols from input files
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->populate_symtab(ctx);
  });

  tbb::parallel_for_each(ctx.dsos, [&](SharedFile<E> *file) {
    file->populate_symtab(ctx);
  });
}

// An ARM64 function with a non-standard calling convention is marked with
// STO_AARCH64_VARIANT_PCS bit in the symbol table.
//
// A function with that bit is not safe to be called through a lazy PLT
// stub because the PLT resolver may clobber registers that should be
// preserved in a non-standard calling convention.
//
// To solve the problem, the dynamic linker scans the dynamic symbol table
// at process startup time and resolve symbols with STO_AARCH64_VARIANT_PCS
// bit eagerly, so that the PLT resolver won't be called for that symbol
// lazily. As an optimization, it does so only when DT_AARCH64_VARIANT_PCS
// is set in the dynamic section.
//
// This function returns true if DT_AARCH64_VARIANT_PCS needs to be set.
static bool contains_variant_pcs(Context<ARM64> &ctx) {
  for (Symbol<ARM64> *sym : ctx.plt->symbols)
    if (sym->esym().arm64_variant_pcs)
      return true;
  return false;
}

template <typename E>
static std::vector<Word<E>> create_dynamic_section(Context<E> &ctx) {
  std::vector<Word<E>> vec;

  auto define = [&](u64 tag, u64 val) {
    vec.push_back(tag);
    vec.push_back(val);
  };

  for (SharedFile<E> *file : ctx.dsos)
    define(DT_NEEDED, ctx.dynstr->find_string(file->soname));

  if (!ctx.arg.rpaths.empty())
    define(ctx.arg.enable_new_dtags ? DT_RUNPATH : DT_RPATH,
           ctx.dynstr->find_string(ctx.arg.rpaths));

  if (!ctx.arg.soname.empty())
    define(DT_SONAME, ctx.dynstr->find_string(ctx.arg.soname));

  for (std::string_view str : ctx.arg.auxiliary)
    define(DT_AUXILIARY, ctx.dynstr->find_string(str));

  for (std::string_view str : ctx.arg.filter)
    define(DT_FILTER, ctx.dynstr->find_string(str));

  if (ctx.reldyn->shdr.sh_size) {
    define(E::is_rela ? DT_RELA : DT_REL, ctx.reldyn->shdr.sh_addr);
    define(E::is_rela ? DT_RELASZ : DT_RELSZ, ctx.reldyn->shdr.sh_size);
    define(E::is_rela ? DT_RELAENT : DT_RELENT, sizeof(ElfRel<E>));
  }

  if (ctx.relrdyn) {
    define(DT_RELR, ctx.relrdyn->shdr.sh_addr);
    define(DT_RELRSZ, ctx.relrdyn->shdr.sh_size);
    define(DT_RELRENT, ctx.relrdyn->shdr.sh_entsize);
  }

  if (ctx.relplt->shdr.sh_size) {
    define(DT_JMPREL, ctx.relplt->shdr.sh_addr);
    define(DT_PLTRELSZ, ctx.relplt->shdr.sh_size);
    define(DT_PLTREL, E::is_rela ? DT_RELA : DT_REL);
  }

  if constexpr (is_sparc<E>) {
    if (ctx.plt->shdr.sh_size)
      define(DT_PLTGOT, ctx.plt->shdr.sh_addr);
  } else if constexpr (is_ppc32<E>) {
    if (ctx.gotplt->shdr.sh_size)
      define(DT_PLTGOT, ctx.gotplt->shdr.sh_addr + GotPltSection<E>::HDR_SIZE);
  } else {
    if (ctx.gotplt->shdr.sh_size)
      define(DT_PLTGOT, ctx.gotplt->shdr.sh_addr);
  }

  if (ctx.dynsym->shdr.sh_size) {
    define(DT_SYMTAB, ctx.dynsym->shdr.sh_addr);
    define(DT_SYMENT, sizeof(ElfSym<E>));
  }

  if (ctx.dynstr->shdr.sh_size) {
    define(DT_STRTAB, ctx.dynstr->shdr.sh_addr);
    define(DT_STRSZ, ctx.dynstr->shdr.sh_size);
  }

  if (find_section(ctx, SHT_INIT_ARRAY)) {
    define(DT_INIT_ARRAY, ctx.__init_array_start->value);
    define(DT_INIT_ARRAYSZ,
           ctx.__init_array_end->value - ctx.__init_array_start->value);
  }

  if (find_section(ctx, SHT_PREINIT_ARRAY)) {
    define(DT_PREINIT_ARRAY, ctx.__preinit_array_start->value);
    define(DT_PREINIT_ARRAYSZ,
           ctx.__preinit_array_end->value - ctx.__preinit_array_start->value);
  }

  if (find_section(ctx, SHT_FINI_ARRAY)) {
    define(DT_FINI_ARRAY, ctx.__fini_array_start->value);
    define(DT_FINI_ARRAYSZ,
           ctx.__fini_array_end->value - ctx.__fini_array_start->value);
  }

  if (ctx.versym->shdr.sh_size)
    define(DT_VERSYM, ctx.versym->shdr.sh_addr);

  if (ctx.verneed->shdr.sh_size) {
    define(DT_VERNEED, ctx.verneed->shdr.sh_addr);
    define(DT_VERNEEDNUM, ctx.verneed->shdr.sh_info);
  }

  if (ctx.verdef) {
    define(DT_VERDEF, ctx.verdef->shdr.sh_addr);
    define(DT_VERDEFNUM, ctx.verdef->shdr.sh_info);
  }

  if (Symbol<E> &sym = *ctx.arg.init;
      sym.file && !sym.file->is_dso)
    define(DT_INIT, sym.get_addr(ctx));

  if (Symbol<E> &sym = *ctx.arg.fini;
      sym.file && !sym.file->is_dso)
    define(DT_FINI, sym.get_addr(ctx));

  if (ctx.hash)
    define(DT_HASH, ctx.hash->shdr.sh_addr);
  if (ctx.gnu_hash)
    define(DT_GNU_HASH, ctx.gnu_hash->shdr.sh_addr);
  if (ctx.has_textrel)
    define(DT_TEXTREL, 0);

  i64 flags = 0;
  i64 flags1 = 0;

  if (ctx.arg.pie)
    flags1 |= DF_1_PIE;

  if (ctx.arg.z_now) {
    flags |= DF_BIND_NOW;
    flags1 |= DF_1_NOW;
  }

  if (ctx.arg.z_origin) {
    flags |= DF_ORIGIN;
    flags1 |= DF_1_ORIGIN;
  }

  if (!ctx.arg.z_dlopen)
    flags1 |= DF_1_NOOPEN;
  if (ctx.arg.z_nodefaultlib)
    flags1 |= DF_1_NODEFLIB;
  if (!ctx.arg.z_delete)
    flags1 |= DF_1_NODELETE;
  if (!ctx.arg.z_dump)
    flags1 |= DF_1_NODUMP;
  if (ctx.arg.z_initfirst)
    flags1 |= DF_1_INITFIRST;
  if (ctx.arg.z_interpose)
    flags1 |= DF_1_INTERPOSE;

  if (!ctx.got->gottp_syms.empty())
    flags |= DF_STATIC_TLS;
  if (ctx.has_textrel)
    flags |= DF_TEXTREL;

  if (flags)
    define(DT_FLAGS, flags);
  if (flags1)
    define(DT_FLAGS_1, flags1);

  if constexpr (is_arm64<E>)
    if (contains_variant_pcs(ctx))
      define(DT_AARCH64_VARIANT_PCS, 1);

  if constexpr (is_ppc32<E>)
    define(DT_PPC_GOT, ctx.gotplt->shdr.sh_addr);

  if constexpr (is_ppc64<E>) {
    // PPC64_GLINK is defined by the psABI to refer 32 bytes before
    // the first PLT entry. I don't know why it's 32 bytes off, but
    // it's what it is.
    define(DT_PPC64_GLINK, ctx.plt->shdr.sh_addr + E::plt_hdr_size - 32);
  }

  // GDB needs a DT_DEBUG entry in an executable to store a word-size
  // data for its own purpose. Its content is not important.
  if (!ctx.arg.shared)
    define(DT_DEBUG, 0);

  define(DT_NULL, 0);

  for (i64 i = 0; i < ctx.arg.spare_dynamic_tags; i++)
    define(DT_NULL, 0);

  return vec;
}

template <typename E>
void DynamicSection<E>::update_shdr(Context<E> &ctx) {
  if (ctx.arg.is_static && !ctx.arg.pie)
    return;

  this->shdr.sh_size = create_dynamic_section(ctx).size() * sizeof(Word<E>);
  this->shdr.sh_link = ctx.dynstr->shndx;
}

template <typename E>
void DynamicSection<E>::copy_buf(Context<E> &ctx) {
  std::vector<Word<E>> contents = create_dynamic_section(ctx);
  assert(this->shdr.sh_size == contents.size() * sizeof(contents[0]));
  write_vector(ctx.buf + this->shdr.sh_offset, contents);
}

template <typename E>
void OutputSection<E>::copy_buf(Context<E> &ctx) {
  if (this->shdr.sh_type != SHT_NOBITS)
    write_to(ctx, ctx.buf + this->shdr.sh_offset);
}

template <typename E>
void OutputSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  tbb::parallel_for((i64)0, (i64)members.size(), [&](i64 i) {
    // Copy section contents to an output file.
    InputSection<E> &isec = *members[i];
    isec.write_to(ctx, buf + isec.offset);

    // Clear trailing padding. We write trap or nop instructions for
    // an executable segment so that a disassembler wouldn't try to
    // disassemble garbage as instructions.
    u64 this_end = isec.offset + isec.sh_size;
    u64 next_start;
    if (i + 1 < members.size())
      next_start = members[i + 1]->offset;
    else
      next_start = this->shdr.sh_size;

    u8 *loc = buf + this_end;
    i64 size = next_start - this_end;

    if (this->shdr.sh_flags & SHF_EXECINSTR) {
      for (i64 i = 0; i + sizeof(E::filler) <= size; i += sizeof(E::filler))
        memcpy(loc + i, E::filler, sizeof(E::filler));
    } else {
      memset(loc, 0, size);
    }
  });

  if constexpr (needs_thunk<E>) {
    tbb::parallel_for_each(thunks, [&](std::unique_ptr<Thunk<E>> &thunk) {
      thunk->copy_buf(ctx);
    });
  }
}

// .relr.dyn contains base relocations encoded in a space-efficient form.
// The contents of the section is essentially just a list of addresses
// that have to be fixed up at runtime.
//
// Here is the encoding scheme (we assume 64-bit ELF in this description
// for the sake of simplicity): .relr.dyn contains zero or more address
// groups. Each address group consists of a 64-bit start address followed
// by zero or more 63-bit bitmaps. Let A be the address of a start
// address. Then, the loader fixes address A. If Nth bit in the following
// bitmap is on, the loader also fixes address A + N * 8. In this scheme,
// one address and one bitmap can represent up to 64 base relocations in a
// 512 bytes range.
//
// A start address and a bitmap is distinguished by the lowest significant
// bit. An address must be even and thus its LSB is 0 (odd address is not
// representable in this encoding and such relocation must be stored to
// the .rel.dyn section). A bitmap has LSB 1.
template <typename E>
static std::vector<u64> encode_relr(std::span<u64> pos) {
  std::vector<u64> vec;
  i64 num_bits = E::is_64 ? 63 : 31;
  i64 max_delta = sizeof(Word<E>) * num_bits;

  for (i64 i = 0; i < pos.size();) {
    assert(i == 0 || pos[i - 1] < pos[i]);
    assert(pos[i] % sizeof(Word<E>) == 0);

    vec.push_back(pos[i]);
    u64 base = pos[i] + sizeof(Word<E>);
    i++;

    for (;;) {
      u64 bits = 0;
      for (; i < pos.size() && pos[i] - base < max_delta; i++) {
        assert(pos[i - 1] < pos[i]);
        assert(pos[i] % sizeof(Word<E>) == 0);
        bits |= (u64)1 << ((pos[i] - base) / sizeof(Word<E>));
      }

      if (!bits)
        break;

      vec.push_back((bits << 1) | 1);
      base += max_delta;
    }
  }
  return vec;
}

template <typename E>
void OutputSection<E>::construct_relr(Context<E> &ctx) {
  if (!ctx.arg.pic)
    return;
  if (!(this->shdr.sh_flags & SHF_ALLOC))
    return;
  if (this->shdr.sh_addralign % sizeof(Word<E>))
    return;

  // Skip it if it is a text section because .text doesn't usually
  // contain any dynamic relocations.
  if (this->shdr.sh_flags & SHF_EXECINSTR)
    return;

  // Collect base relocations
  std::vector<std::vector<u64>> shards(members.size());

  tbb::parallel_for((i64)0, (i64)members.size(), [&](i64 i) {
    InputSection<E> &isec = *members[i];

    if (isec.shdr().sh_addralign % sizeof(Word<E>) == 0)
      for (const ElfRel<E> &r : isec.get_rels(ctx))
        if (r.r_type == E::R_ABS && r.r_offset % sizeof(Word<E>) == 0)
          if (Symbol<E> &sym = *isec.file.symbols[r.r_sym];
              !sym.is_ifunc() && !sym.is_absolute() && !sym.is_imported)
            shards[i].push_back(isec.offset + r.r_offset);
  });

  // Compress them
  std::vector<u64> pos = flatten(shards);
  this->relr = encode_relr<E>(pos);
}

// Compute spaces needed for thunk symbols
template <typename E>
void OutputSection<E>::compute_symtab_size(Context<E> &ctx) {
  if constexpr (needs_thunk<E>) {
    this->strtab_size = 0;
    this->num_local_symtab = 0;

    for (std::unique_ptr<Thunk<E>> &thunk : thunks) {
      // For ARM32, we emit additional symbol "$t", "$a" and "$d" for
      // each thunk to mark the beginning of Thumb code, ARM code and
      // data, respectively.
      if constexpr (is_arm32<E>)
        this->num_local_symtab += thunk->symbols.size() * 4;
      else
        this->num_local_symtab += thunk->symbols.size();

      for (Symbol<E> *sym : thunk->symbols)
        this->strtab_size += sym->name().size() + sizeof("$thunk");
    }
  }
}

// If we create range extension thunks, we also synthesize symbols to mark
// the locations of thunks. Creating such symbols is optional, but it helps
// disassembling and/or debugging our output.
template <typename E>
void OutputSection<E>::populate_symtab(Context<E> &ctx) {
  if constexpr (needs_thunk<E>) {
    ElfSym<E> *esym =
      (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset) + this->local_symtab_idx;

    u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
    u8 *strtab = strtab_base + this->strtab_offset;

    auto write_esym = [&](u64 addr, i64 st_name) {
      memset(esym, 0, sizeof(*esym));
      esym->st_name = st_name;
      esym->st_type = STT_FUNC;
      esym->st_shndx = this->shndx;
      esym->st_value = addr;
      esym++;
    };

    for (std::unique_ptr<Thunk<E>> &thunk : thunks) {
      for (i64 i = 0; i < thunk->symbols.size(); i++) {
        Symbol<E> &sym = *thunk->symbols[i];
        u64 addr = thunk->get_addr(i);

        write_esym(addr, strtab - strtab_base);

        strtab += write_string(strtab, sym.name()) - 1;
        strtab += write_string(strtab, "$thunk");

        // Emit "$t", "$a" and "$d" if ARM32.
        if constexpr (is_arm32<E>) {
          write_esym(addr, ctx.strtab->THUMB);
          write_esym(addr + 4, ctx.strtab->ARM);
          write_esym(addr + 12, ctx.strtab->DATA);
        }
      }
    }
  }
}

template <typename E>
void GotSection<E>::add_got_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_got_idx(ctx, this->shdr.sh_size / sizeof(Word<E>));

  // An IFUNC symbol uses two GOT slots in a position-dependent
  // executable.
  if (sym->is_pde_ifunc(ctx))
    this->shdr.sh_size += sizeof(Word<E>) * 2;
  else
    this->shdr.sh_size += sizeof(Word<E>);

  got_syms.push_back(sym);
}

template <typename E>
void GotSection<E>::add_gottp_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_gottp_idx(ctx, this->shdr.sh_size / sizeof(Word<E>));
  this->shdr.sh_size += sizeof(Word<E>);
  gottp_syms.push_back(sym);
}

template <typename E>
void GotSection<E>::add_tlsgd_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_tlsgd_idx(ctx, this->shdr.sh_size / sizeof(Word<E>));
  this->shdr.sh_size += sizeof(Word<E>) * 2;
  tlsgd_syms.push_back(sym);
}

template <typename E>
void GotSection<E>::add_tlsdesc_symbol(Context<E> &ctx, Symbol<E> *sym) {
  // TLSDESC's GOT slot values may vary depending on libc, so we
  // always emit a dynamic relocation for each TLSDESC entry.
  //
  // If dynamic relocation is not available (i.e. if we are creating a
  // statically-linked executable), we always relax TLSDESC relocations
  // so that no TLSDESC relocation exist at runtime.
  assert(supports_tlsdesc<E>);
  assert(!ctx.arg.is_static);

  sym->set_tlsdesc_idx(ctx, this->shdr.sh_size / sizeof(Word<E>));
  this->shdr.sh_size += sizeof(Word<E>) * 2;
  tlsdesc_syms.push_back(sym);
}

template <typename E>
void GotSection<E>::add_tlsld(Context<E> &ctx) {
  assert(tlsld_idx == -1);
  tlsld_idx = this->shdr.sh_size / sizeof(Word<E>);
  this->shdr.sh_size += sizeof(Word<E>) * 2;
}

template <typename E>
u64 GotSection<E>::get_tlsld_addr(Context<E> &ctx) const {
  assert(tlsld_idx != -1);
  return this->shdr.sh_addr + tlsld_idx * sizeof(Word<E>);
}

namespace {
template <typename E>
struct GotEntry {
  bool is_relr(Context<E> &ctx) const {
    return r_type == E::R_RELATIVE && ctx.arg.pack_dyn_relocs_relr;
  }

  i64 idx = 0;
  u64 val = 0;
  i64 r_type = R_NONE;
  Symbol<E> *sym = nullptr;
};
}

// Get .got and .rel.dyn contents.
//
// .got is a linker-synthesized constant pool whose entry is of pointer
// size. If we know a correct value for an entry, we'll just set that value
// to the entry. Otherwise, we'll create a dynamic relocation and let the
// dynamic linker to fill the entry at load-time.
//
// Most GOT entries contain addresses of global variable. If a global
// variable is an imported symbol, we don't know its address until runtime.
// GOT contains the addresses of such variables at runtime so that we can
// access imported global variables via GOT.
//
// Thread-local variables (TLVs) also use GOT entries. We need them because
// TLVs are accessed in a different way than the ordinary global variables.
// Their addresses are not unique; each thread has its own copy of TLVs.
template <typename E>
static std::vector<GotEntry<E>> get_got_entries(Context<E> &ctx) {
  std::vector<GotEntry<E>> entries;
  auto add = [&](GotEntry<E> ent) { entries.push_back(ent); };

  // Create GOT entries for ordinary symbols
  for (Symbol<E> *sym : ctx.got->got_syms) {
    i64 idx = sym->get_got_idx(ctx);

    // If a symbol is imported, let the dynamic linker to resolve it.
    if (sym->is_imported) {
      add({idx, 0, E::R_GLOB_DAT, sym});
      continue;
    }

    // IFUNC always needs to be fixed up by the dynamic linker.
    if constexpr (supports_ifunc<E>) {
      if (sym->is_ifunc()) {
        if (sym->is_pde_ifunc(ctx)) {
          add({idx, sym->get_plt_addr(ctx)});
          add({idx + 1, sym->get_addr(ctx, NO_PLT), E::R_IRELATIVE});
        } else {
          add({idx, sym->get_addr(ctx, NO_PLT), E::R_IRELATIVE});
        }
        continue;
      }
    }

    // If we know an address at link-time, fill that GOT entry now.
    // It may need a base relocation, though.
    if (ctx.arg.pic && sym->is_relative())
      add({idx, sym->get_addr(ctx, NO_PLT), E::R_RELATIVE});
    else
      add({idx, sym->get_addr(ctx, NO_PLT)});
  }

  // Create GOT entries for TLVs.
  for (Symbol<E> *sym : ctx.got->tlsgd_syms) {
    i64 idx = sym->get_tlsgd_idx(ctx);

    if (sym->is_imported) {
      // If a symbol is imported, let the dynamic linker to resolve it.
      add({idx, 0, E::R_DTPMOD, sym});
      add({idx + 1, 0, E::R_DTPOFF, sym});
    } else if (ctx.arg.shared) {
      // If we are creating a shared library, we know the TLV's offset
      // within the current TLS block. We don't know the module ID though.
      add({idx, 0, E::R_DTPMOD});
      add({idx + 1, sym->get_addr(ctx) - ctx.dtp_addr});
    } else {
      // If we are creating an executable, we know both the module ID and
      // the offset. Module ID 1 indicates the main executable.
      add({idx, 1});
      add({idx + 1, sym->get_addr(ctx) - ctx.dtp_addr});
    }
  }

  if constexpr (supports_tlsdesc<E>) {
    for (Symbol<E> *sym : ctx.got->tlsdesc_syms) {
      i64 idx = sym->get_tlsdesc_idx(ctx);

      // TLSDESC uses two consecutive GOT slots, and a single TLSDESC
      // dynamic relocation fills both. The actual values of the slots
      // vary depending on libc, so we can't precompute their values.
      // We always emit a dynamic relocation for each incoming TLSDESC
      // reloc.
      if (sym->is_imported)
        add({idx, 0, E::R_TLSDESC, sym});
      else
        add({idx, sym->get_addr(ctx) - ctx.tls_begin, E::R_TLSDESC});
    }
  }

  for (Symbol<E> *sym : ctx.got->gottp_syms) {
    i64 idx = sym->get_gottp_idx(ctx);

    if (sym->is_imported) {
      // If we know nothing about the symbol, let the dynamic linker
      // to fill the GOT entry.
      add({idx, 0, E::R_TPOFF, sym});
    } else if (ctx.arg.shared) {
      // If we know the offset within the current thread vector,
      // let the dynamic linker to adjust it.
      add({idx, sym->get_addr(ctx) - ctx.tls_begin, E::R_TPOFF});
    } else {
      // Otherwise, we know the offset from the thread pointer (TP) at
      // link-time, so we can fill the GOT entry directly.
      add({idx, sym->get_addr(ctx) - ctx.tp_addr});
    }
  }

  if (ctx.got->tlsld_idx != -1) {
    if (ctx.arg.shared)
      add({ctx.got->tlsld_idx, 0, E::R_DTPMOD});
    else
      add({ctx.got->tlsld_idx, 1}); // 1 means the main executable
  }

  return entries;
}

template <typename E>
i64 GotSection<E>::get_reldyn_size(Context<E> &ctx) const {
  i64 n = 0;
  for (GotEntry<E> &ent : get_got_entries(ctx))
    if (!ent.is_relr(ctx) && ent.r_type != R_NONE)
      n++;
  return n;
}

// Fill .got and .rel.dyn.
template <typename E>
void GotSection<E>::copy_buf(Context<E> &ctx) {
  Word<E> *buf = (Word<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  // s390x psABI requires GOT[0] to be set to the link-time value of _DYNAMIC.
  if constexpr (is_s390x<E>)
    if (ctx.dynamic)
      buf[0] = ctx.dynamic->shdr.sh_addr;

  // arm64 psABI doesn't say anything about GOT[0], but glibc/arm64's code
  // path for -static-pie wrongly assumed that GOT[0] refers _DYNAMIC.
  //
  // https://sourceware.org/git/?p=glibc.git;a=commitdiff;h=43d06ed218fc8be5
  if constexpr (is_arm64<E>)
    if (ctx.dynamic && ctx.arg.is_static && ctx.arg.pie)
      buf[0] = ctx.dynamic->shdr.sh_addr;

  ElfRel<E> *rel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                                 this->reldyn_offset);

  for (GotEntry<E> &ent : get_got_entries(ctx)) {
    if (ent.is_relr(ctx) || ent.r_type == R_NONE) {
      buf[ent.idx] = ent.val;
      continue;
    }

    *rel++ = ElfRel<E>(this->shdr.sh_addr + ent.idx * sizeof(Word<E>),
                       ent.r_type,
                       ent.sym ? ent.sym->get_dynsym_idx(ctx) : 0,
                       ent.val);

    bool is_tlsdesc = false;
    if constexpr (supports_tlsdesc<E>)
      is_tlsdesc = (ent.r_type == E::R_TLSDESC);

    if (ctx.arg.apply_dynamic_relocs) {
      if (is_tlsdesc && !is_arm32<E>) {
        // A single TLSDESC relocation fixes two consecutive GOT slots
        // where one slot holds a function pointer and the other an
        // argument to the function. An addend should be applied not to
        // the function pointer but to the function argument, which is
        // usually stored to the second slot.
        //
        // ARM32 employs the inverted layout for some reason, so an
        // addend is applied to the first slot.
        buf[ent.idx + 1] = ent.val;
      } else {
        buf[ent.idx] = ent.val;
      }
    }
  }
}

template <typename E>
void GotSection<E>::construct_relr(Context<E> &ctx) {
  assert(ctx.arg.pack_dyn_relocs_relr);

  std::vector<u64> pos;
  for (GotEntry<E> &ent : get_got_entries(ctx))
    if (ent.is_relr(ctx))
      pos.push_back(ent.idx * sizeof(Word<E>));

  this->relr = encode_relr<E>(pos);
}

template <typename E>
void GotSection<E>::compute_symtab_size(Context<E> &ctx) {
  this->strtab_size = 0;
  this->num_local_symtab = 0;

  for (Symbol<E> *sym : got_syms) {
    this->strtab_size += sym->name().size() + sizeof("$got");
    this->num_local_symtab++;
  }

  for (Symbol<E> *sym : gottp_syms) {
    this->strtab_size += sym->name().size() + sizeof("$gottp");
    this->num_local_symtab++;
  }

  for (Symbol<E> *sym : tlsgd_syms) {
    this->strtab_size += sym->name().size() + sizeof("$tlsgd");
    this->num_local_symtab++;
  }

  for (Symbol<E> *sym : tlsdesc_syms) {
    this->strtab_size += sym->name().size() + sizeof("$tlsdesc");
    this->num_local_symtab++;
  }

  if (tlsld_idx != -1) {
    this->strtab_size += sizeof("$tlsld");
    this->num_local_symtab++;
  }
}

template <typename E>
void GotSection<E>::populate_symtab(Context<E> &ctx) {
  if (this->num_local_symtab == 0)
    return;

  ElfSym<E> *esym =
    (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset) + this->local_symtab_idx;

  u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
  u8 *strtab = strtab_base + this->strtab_offset;

  auto write = [&](std::string_view name, std::string_view suffix, i64 value) {
    memset(esym, 0, sizeof(*esym));
    esym->st_name = strtab - strtab_base;
    esym->st_type = STT_OBJECT;
    esym->st_shndx = this->shndx;
    esym->st_value = value;
    esym++;

    strtab += write_string(strtab, name) - 1;
    strtab += write_string(strtab, suffix);
  };

  for (Symbol<E> *sym : got_syms)
    write(sym->name(), "$got", sym->get_got_addr(ctx));

  for (Symbol<E> *sym : gottp_syms)
    write(sym->name(), "$gottp", sym->get_gottp_addr(ctx));

  for (Symbol<E> *sym : tlsgd_syms)
    write(sym->name(), "$tlsgd", sym->get_tlsgd_addr(ctx));

  for (Symbol<E> *sym : tlsdesc_syms)
    write(sym->name(), "$tlsdesc", sym->get_tlsdesc_addr(ctx));

  if (tlsld_idx != -1)
    write("", "$tlsld", get_tlsld_addr(ctx));
}

template <typename E>
void GotPltSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = HDR_SIZE + ctx.plt->symbols.size() * ENTRY_SIZE;
}

template <typename E>
void GotPltSection<E>::copy_buf(Context<E> &ctx) {
  // On PPC64, it's dynamic loader responsibility to fill the .got.plt
  // section. Dynamic loader finds the address of the first PLT entry by
  // DT_PPC64_GLINK and assumes that each PLT entry is 4 bytes long.
  if constexpr (!is_ppc64<E>) {
    Word<E> *buf = (Word<E> *)(ctx.buf + this->shdr.sh_offset);

    // The first slot of .got.plt points to _DYNAMIC, as requested by
    // the psABI. The second and the third slots are reserved by the psABI.
    static_assert(HDR_SIZE / sizeof(Word<E>) == 3);

    buf[0] = ctx.dynamic ? (u64)ctx.dynamic->shdr.sh_addr : 0;
    buf[1] = 0;
    buf[2] = 0;

    for (i64 i = 0; i < ctx.plt->symbols.size(); i++)
      buf[i + 3] = ctx.plt->shdr.sh_addr;
  }
}

template <typename E>
void PltSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(!sym->has_plt(ctx));
  sym->set_plt_idx(ctx, symbols.size());
  symbols.push_back(sym);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void PltSection<E>::update_shdr(Context<E> &ctx) {
  if (symbols.empty())
    this->shdr.sh_size = 0;
  else
    this->shdr.sh_size = to_plt_offset<E>(symbols.size());
}

template <typename E>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + ctx.plt->shdr.sh_offset;
  write_plt_header(ctx, buf);

  for (i64 i = 0; i < symbols.size(); i++)
    write_plt_entry(ctx, buf + to_plt_offset<E>(i), *symbols[i]);
}

template <typename E>
void PltSection<E>::compute_symtab_size(Context<E> &ctx) {
  this->num_local_symtab = symbols.size();
  this->strtab_size = 0;

  for (Symbol<E> *sym : symbols)
    this->strtab_size += sym->name().size() + sizeof("$plt");

  if constexpr (is_arm32<E>)
    this->num_local_symtab += symbols.size() * 2 + 2;
}

template <typename E>
void PltSection<E>::populate_symtab(Context<E> &ctx) {
  if (this->num_local_symtab == 0)
    return;

  ElfSym<E> *esym =
    (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset) + this->local_symtab_idx;

  u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
  u8 *strtab = strtab_base + this->strtab_offset;

  auto write_esym = [&](u64 addr, i64 st_name) {
    memset(esym, 0, sizeof(*esym));
    esym->st_name = st_name;
    esym->st_type = STT_FUNC;
    esym->st_shndx = this->shndx;
    esym->st_value = addr;
    esym++;
  };

  if constexpr (is_arm32<E>) {
    write_esym(this->shdr.sh_addr, ctx.strtab->ARM);
    write_esym(this->shdr.sh_addr + 16, ctx.strtab->DATA);
  }

  for (Symbol<E> *sym : symbols) {
    u64 addr = sym->get_plt_addr(ctx);
    write_esym(addr, strtab - strtab_base);
    strtab += write_string(strtab, sym->name()) - 1;
    strtab += write_string(strtab, "$plt");

    if constexpr (is_arm32<E>) {
      write_esym(addr, ctx.strtab->ARM);
      write_esym(addr + 12, ctx.strtab->DATA);
    }
  }
}

template <typename E>
void PltGotSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(!sym->has_plt(ctx));
  assert(sym->has_got(ctx));

  sym->set_pltgot_idx(ctx, symbols.size());
  symbols.push_back(sym);
  this->shdr.sh_size = symbols.size() * E::pltgot_size;
}

template <typename E>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + ctx.pltgot->shdr.sh_offset;
  for (i64 i = 0; i < symbols.size(); i++)
    write_pltgot_entry(ctx, buf + i * E::pltgot_size, *symbols[i]);
}

template <typename E>
void PltGotSection<E>::compute_symtab_size(Context<E> &ctx) {
  this->num_local_symtab = symbols.size();
  this->strtab_size = 0;

  for (Symbol<E> *sym : symbols)
    this->strtab_size += sym->name().size() + sizeof("$pltgot");

  if constexpr (is_arm32<E>)
    this->num_local_symtab += symbols.size() * 2;
}

template <typename E>
void PltGotSection<E>::populate_symtab(Context<E> &ctx) {
  if (this->num_local_symtab == 0)
    return;

  ElfSym<E> *esym =
    (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset) + this->local_symtab_idx;

  u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
  u8 *strtab = strtab_base + this->strtab_offset;

  auto write_esym = [&](u64 addr, i64 st_name) {
    memset(esym, 0, sizeof(*esym));
    esym->st_name = st_name;
    esym->st_type = STT_FUNC;
    esym->st_shndx = this->shndx;
    esym->st_value = addr;
    esym++;
  };

  for (Symbol<E> *sym : symbols) {
    u64 addr = sym->get_plt_addr(ctx);
    write_esym(addr, strtab - strtab_base);
    strtab += write_string(strtab, sym->name()) - 1;
    strtab += write_string(strtab, "$pltgot");

    if constexpr (is_arm32<E>) {
      write_esym(addr, ctx.strtab->ARM);
      write_esym(addr + 12, ctx.strtab->DATA);
    }
  }
}

template <typename E>
void RelPltSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = ctx.plt->symbols.size() * sizeof(ElfRel<E>);
  this->shdr.sh_link = ctx.dynsym->shndx;

  if (!is_sparc<E>)
    this->shdr.sh_info = ctx.gotplt->shndx;
}

template <typename E>
void RelPltSection<E>::copy_buf(Context<E> &ctx) {
  ElfRel<E> *buf = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset);

  for (Symbol<E> *sym : ctx.plt->symbols) {
    // SPARC doesn't have a .got.plt because its role is merged to .plt.
    // On SPARC, .plt is writable (!) and the dynamic linker directly
    // modifies .plt's machine instructions as it resolves dynamic symbols.
    // Therefore, it doesn't need a separate section to store the symbol
    // resolution results. That is of course horrible from the security
    // point of view, though.
    u64 addr = is_sparc<E> ? sym->get_plt_addr(ctx) : sym->get_gotplt_addr(ctx);
    *buf++ = ElfRel<E>(addr, E::R_JUMP_SLOT, sym->get_dynsym_idx(ctx), 0);
  }
}

template <typename E>
ElfSym<E> to_output_esym(Context<E> &ctx, Symbol<E> &sym, u32 st_name,
                         U32<E> *shn_xindex) {
  ElfSym<E> esym;
  memset(&esym, 0, sizeof(esym));

  esym.st_name = st_name;
  esym.st_type = sym.get_type();
  esym.st_size = sym.esym().st_size;

  if (sym.is_local(ctx))
    esym.st_bind = STB_LOCAL;
  else if (sym.is_weak)
    esym.st_bind = STB_WEAK;
  else if (sym.file->is_dso)
    esym.st_bind = STB_GLOBAL;
  else
    esym.st_bind = sym.esym().st_bind;

  if constexpr (is_arm64<E>)
    esym.arm64_variant_pcs = sym.esym().arm64_variant_pcs;

  if constexpr (is_ppc64v2<E>)
    esym.ppc_local_entry = sym.esym().ppc_local_entry;

  if constexpr (is_alpha<E>)
    esym.alpha_st_other = sym.esym().alpha_st_other;

  auto get_st_shndx = [&](Symbol<E> &sym) -> u32 {
    if (SectionFragment<E> *frag = sym.get_frag())
      if (frag->is_alive)
        return frag->output_section.shndx;

    if constexpr (is_ppc64v1<E>)
      if (sym.has_opd(ctx))
        return ctx.extra.opd->shndx;

    if (InputSection<E> *isec = sym.get_input_section()) {
      if (isec->is_alive)
        return isec->output_section->shndx;
      else if (isec->is_killed_by_icf())
        return isec->leader->output_section->shndx;
    }

    return SHN_UNDEF;
  };

  i64 shndx = -1;
  if (sym.has_copyrel) {
    shndx = sym.is_copyrel_readonly ? ctx.copyrel_relro->shndx : ctx.copyrel->shndx;
    esym.st_value = sym.get_addr(ctx);
  } else if (sym.file->is_dso || sym.esym().is_undef()) {
    esym.st_shndx = SHN_UNDEF;
    esym.st_size = 0;
    if (sym.is_canonical)
      esym.st_value = sym.get_plt_addr(ctx);
  } else if (Chunk<E> *osec = sym.get_output_section()) {
    // Linker-synthesized symbols
    shndx = osec->shndx;
    esym.st_value = sym.get_addr(ctx);
  } else if (SectionFragment<E> *frag = sym.get_frag()) {
    // Section fragment
    shndx = frag->output_section.shndx;
    esym.st_value = sym.get_addr(ctx);
  } else if (!sym.get_input_section()) {
    // Absolute symbol
    esym.st_shndx = SHN_ABS;
    esym.st_value = sym.get_addr(ctx);
  } else if (sym.get_type() == STT_TLS) {
    // TLS symbol
    shndx = get_st_shndx(sym);
    esym.st_value = sym.get_addr(ctx) - ctx.tls_begin;
  } else if (sym.is_pde_ifunc(ctx)) {
    // IFUNC symbol in PDE that uses two GOT slots
    shndx = get_st_shndx(sym);
    esym.st_type = STT_FUNC;
    esym.st_visibility = sym.visibility;
    esym.st_value = sym.get_plt_addr(ctx);
  } else {
    shndx = get_st_shndx(sym);
    esym.st_visibility = sym.visibility;
    esym.st_value = sym.get_addr(ctx, NO_PLT);
  }

  // Symbol's st_shndx is only 16 bits wide, so we can't store a large
  // section index there. If the total number of sections is equal to
  // or greater than SHN_LORESERVE (= 65280), the real index is stored
  // to a SHT_SYMTAB_SHNDX section which contains a parallel array of
  // the symbol table.
  if (0 <= shndx && shndx < SHN_LORESERVE) {
    esym.st_shndx = shndx;
  } else if (SHN_LORESERVE <= shndx) {
    assert(shn_xindex);
    esym.st_shndx = SHN_XINDEX;
    *shn_xindex = shndx;
  }

  return esym;
}

template <typename E>
void DynsymSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(!finalized);

  if (symbols.empty())
    symbols.resize(1);

  if (sym->get_dynsym_idx(ctx) == -1) {
    sym->set_dynsym_idx(ctx, -2);
    symbols.push_back(sym);
  }
}

template <typename E>
void DynsymSection<E>::finalize(Context<E> &ctx) {
  Timer t(ctx, "DynsymSection::finalize");
  assert(!finalized);
  finalized = true;

  if (symbols.empty())
    return;

  // Sort symbols. In any symtab, local symbols must precede global symbols.
  auto first_global = std::stable_partition(symbols.begin() + 1, symbols.end(),
                                            [&](Symbol<E> *sym) {
    return sym->is_local(ctx);
  });

  // We also place undefined symbols before defined symbols for .gnu.hash.
  // Defined symbols are sorted by their hashes for .gnu.hash.
  if (ctx.gnu_hash) {
    // Count the number of exported symbols to compute the size of .gnu.hash.
    i64 num_exported = 0;
    for (i64 i = 1; i < symbols.size(); i++)
      if (symbols[i]->is_exported)
        num_exported++;

    u32 num_buckets = num_exported / ctx.gnu_hash->LOAD_FACTOR + 1;
    ctx.gnu_hash->num_buckets = num_buckets;

    tbb::parallel_for_each(first_global, symbols.end(), [&](Symbol<E> *sym) {
      sym->set_djb_hash(ctx, djb_hash(sym->name()));
    });

    tbb::parallel_sort(first_global, symbols.end(),
                       [&](Symbol<E> *a, Symbol<E> *b) {
      if (a->is_exported != b->is_exported)
        return b->is_exported;

      return std::tuple(a->get_djb_hash(ctx) % num_buckets, a->name()) <
             std::tuple(b->get_djb_hash(ctx) % num_buckets, b->name());
    });
  }

  // Compute .dynstr size
  ctx.dynstr->dynsym_offset = ctx.dynstr->shdr.sh_size;

  tbb::enumerable_thread_specific<i64> size;
  tbb::parallel_for((i64)1, (i64)symbols.size(), [&](i64 i) {
    symbols[i]->set_dynsym_idx(ctx, i);
    size.local() += symbols[i]->name().size() + 1;
  });

  ctx.dynstr->shdr.sh_size += size.combine(std::plus());

  // ELF's symbol table sh_info holds the offset of the first global symbol.
  this->shdr.sh_info = first_global - symbols.begin();
}

template <typename E>
void DynsymSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_link = ctx.dynstr->shndx;
  this->shdr.sh_size = sizeof(ElfSym<E>) * symbols.size();
}

template <typename E>
void DynsymSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memset(base, 0, sizeof(ElfSym<E>));
  i64 name_offset = ctx.dynstr->dynsym_offset;

  for (i64 i = 1; i < symbols.size(); i++) {
    Symbol<E> &sym = *symbols[i];
    ElfSym<E> &esym =
      *(ElfSym<E> *)(base + sym.get_dynsym_idx(ctx) * sizeof(ElfSym<E>));

    esym = to_output_esym(ctx, sym, name_offset, nullptr);
    name_offset += sym.name().size() + 1;
    assert(esym.st_bind != STB_LOCAL || i < this->shdr.sh_info);
  }
}

template <typename E>
void HashSection<E>::update_shdr(Context<E> &ctx) {
  if (ctx.dynsym->symbols.empty())
    return;

  i64 header_size = 8;
  i64 num_slots = ctx.dynsym->symbols.size();
  this->shdr.sh_size = header_size + num_slots * 8;
  this->shdr.sh_link = ctx.dynsym->shndx;
}

template <typename E>
void HashSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memset(base, 0, this->shdr.sh_size);

  std::span<Symbol<E> *> syms = ctx.dynsym->symbols;
  U32<E> *hdr = (U32<E> *)base;
  U32<E> *buckets = (U32<E> *)(base + 8);
  U32<E> *chains = buckets + syms.size();

  hdr[0] = hdr[1] = syms.size();

  std::vector<u32> hashes(syms.size());
  tbb::parallel_for((i64)1, (i64)syms.size(), [&](i64 i) {
    hashes[i] = elf_hash(syms[i]->name()) % syms.size();
  });

  for (i64 i = 1; i < syms.size(); i++) {
    i64 h = hashes[i];
    chains[syms[i]->get_dynsym_idx(ctx)] = buckets[h];
    buckets[h] = syms[i]->get_dynsym_idx(ctx);
  }
}

template <typename E>
std::span<Symbol<E> *>
GnuHashSection<E>::get_exported_symbols(Context<E> &ctx) {
  std::span<Symbol<E> *> syms = ctx.dynsym->symbols;
  auto it = std::partition_point(syms.begin() + 1, syms.end(), [](Symbol<E> *sym) {
    return !sym->is_exported;
  });
  return syms.subspan(it - syms.begin());
}

template <typename E>
void GnuHashSection<E>::update_shdr(Context<E> &ctx) {
  if (ctx.dynsym->symbols.empty())
    return;

  this->shdr.sh_link = ctx.dynsym->shndx;

  i64 num_exported = get_exported_symbols(ctx).size();
  if (num_exported) {
    // We allocate 12 bits for each symbol in the bloom filter.
    i64 num_bits = num_exported * 12;
    num_bloom = bit_ceil(num_bits / (sizeof(Word<E>) * 8));
  }

  this->shdr.sh_size = HEADER_SIZE;                  // Header
  this->shdr.sh_size += num_bloom * sizeof(Word<E>); // Bloom filter
  this->shdr.sh_size += num_buckets * 4;             // Hash buckets
  this->shdr.sh_size += num_exported * 4;            // Hash values
}

template <typename E>
void GnuHashSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memset(base, 0, this->shdr.sh_size);

  std::span<Symbol<E> *> syms = get_exported_symbols(ctx);
  std::vector<u32> indices(syms.size());
  i64 exported_offset = ctx.dynsym->symbols.size() - syms.size();

  *(U32<E> *)base = num_buckets;
  *(U32<E> *)(base + 4) = exported_offset;
  *(U32<E> *)(base + 8) = num_bloom;
  *(U32<E> *)(base + 12) = BLOOM_SHIFT;

  // Write a bloom filter
  Word<E> *bloom = (Word<E> *)(base + HEADER_SIZE);

  for (i64 i = 0; i < syms.size(); i++) {
    constexpr i64 word_bits = sizeof(Word<E>) * 8;

    i64 h = syms[i]->get_djb_hash(ctx);
    indices[i] = h % num_buckets;

    i64 idx = (h / word_bits) % num_bloom;
    bloom[idx] |= 1LL << (h % word_bits);
    bloom[idx] |= 1LL << ((h >> BLOOM_SHIFT) % word_bits);
  }

  // Write hash bucket indices
  U32<E> *buckets = (U32<E> *)(bloom + num_bloom);

  for (i64 i = 0; i < syms.size(); i++)
    if (!buckets[indices[i]])
      buckets[indices[i]] = i + exported_offset;

  // Write a hash table
  U32<E> *table = buckets + num_buckets;

  for (i64 i = 0; i < syms.size(); i++) {
    // The last entry in a chain must be terminated with an entry with
    // least-significant bit 1.
    u32 h = syms[i]->get_djb_hash(ctx);
    if (i == syms.size() - 1 || indices[i] != indices[i + 1])
      table[i] = h | 1;
    else
      table[i] = h & ~1;
  }
}

template <typename E>
std::string_view
get_merged_output_name(Context<E> &ctx, std::string_view name, u64 flags,
                       i64 entsize, i64 addralign) {
  if (ctx.arg.relocatable && !ctx.arg.relocatable_merge_sections)
    return name;
  if (ctx.arg.unique && ctx.arg.unique->match(name))
    return name;

  // GCC seems to create sections named ".rodata.strN.<mangled-symbol-name>.M".
  // We want to eliminate the symbol name part from the section name.
  if ((flags & SHF_STRINGS) && name.starts_with(".rodata.")) {
    std::string name2 = ".rodata.str"s + std::to_string(entsize) +
                        "." + std::to_string(addralign);
    if (name == name2)
      return name;
    return save_string(ctx, name2);
  }

  return name;
}

template <typename E>
MergedSection<E>::MergedSection(std::string_view name, i64 flags, i64 type,
                                i64 entsize) {
  this->name = name;
  this->shdr.sh_flags = flags;
  this->shdr.sh_type = type;
  this->shdr.sh_entsize = entsize;
}

template <typename E>
MergedSection<E> *
MergedSection<E>::get_instance(Context<E> &ctx, std::string_view name,
                               i64 type, i64 flags,
                               i64 entsize, i64 addralign) {
  name = get_merged_output_name(ctx, name, flags, entsize, addralign);
  flags = flags & ~(u64)SHF_GROUP & ~(u64)SHF_COMPRESSED;

  auto find = [&]() -> MergedSection * {
    for (std::unique_ptr<MergedSection<E>> &osec : ctx.merged_sections)
      if (name == osec->name && flags == osec->shdr.sh_flags &&
          type == osec->shdr.sh_type && entsize == osec->shdr.sh_entsize)
        return osec.get();
    return nullptr;
  };

  // Search for an exiting output section.
  static std::shared_mutex mu;
  {
    std::shared_lock lock(mu);
    if (MergedSection *osec = find())
      return osec;
  }

  // Create a new output section.
  std::unique_lock lock(mu);
  if (MergedSection *osec = find())
    return osec;

  MergedSection *osec = new MergedSection(name, flags, type, entsize);
  ctx.merged_sections.emplace_back(osec);
  return osec;
}

template <typename E>
SectionFragment<E> *
MergedSection<E>::insert(Context<E> &ctx, std::string_view data, u64 hash,
                         i64 p2align) {
  std::call_once(once_flag, [&] {
    // We aim 2/3 occupation ratio
    map.resize(estimator.get_cardinality() * 3 / 2);
  });

  // Even if GC is enabled, we garbage-collect only memory-mapped strings.
  // Non-memory-allocated strings are typically identifiers used by debug info.
  // To remove such strings, use the `strip` command.
  bool is_alive = !ctx.arg.gc_sections || !(this->shdr.sh_flags & SHF_ALLOC);

  SectionFragment<E> *frag;
  bool inserted;
  std::tie(frag, inserted) =
    map.insert(data, hash, SectionFragment(this, is_alive));
  update_maximum(frag->p2align, p2align);
  return frag;
}

template <typename E>
void MergedSection<E>::assign_offsets(Context<E> &ctx) {
  std::vector<i64> sizes(map.NUM_SHARDS);
  Atomic<i64> alignment = 1;

  tbb::parallel_for((i64)0, map.NUM_SHARDS, [&](i64 i) {
    using Entry = typename decltype(map)::Entry;
    std::vector<Entry *> entries = map.get_sorted_entries(i);

    i64 offset = 0;
    i64 p2align = 0;

    for (Entry *ent : entries) {
      SectionFragment<E> &frag = ent->value;
      if (frag.is_alive) {
        offset = align_to(offset, 1 << frag.p2align);
        frag.offset = offset;
        offset += ent->keylen;
        p2align = std::max<i64>(p2align, frag.p2align);
      }
    }

    sizes[i] = offset;
    update_maximum(alignment, 1 << p2align);

    static Counter merged_strings("merged_strings");
    merged_strings += entries.size();
  });

  i64 shard_size = map.nbuckets / map.NUM_SHARDS;
  shard_offsets.resize(map.NUM_SHARDS + 1);

  for (i64 i = 1; i < map.NUM_SHARDS + 1; i++)
    shard_offsets[i] =
      align_to(shard_offsets[i - 1] + sizes[i - 1], alignment);

  tbb::parallel_for((i64)1, map.NUM_SHARDS, [&](i64 i) {
    for (i64 j = shard_size * i; j < shard_size * (i + 1); j++) {
      SectionFragment<E> &frag = map.entries[j].value;
      if (frag.is_alive)
        frag.offset += shard_offsets[i];
    }
  });

  this->shdr.sh_size = shard_offsets[map.NUM_SHARDS];
  this->shdr.sh_addralign = alignment;
}

template <typename E>
void MergedSection<E>::copy_buf(Context<E> &ctx) {
  write_to(ctx, ctx.buf + this->shdr.sh_offset);
}

template <typename E>
void MergedSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  i64 shard_size = map.nbuckets / map.NUM_SHARDS;

  tbb::parallel_for((i64)0, map.NUM_SHARDS, [&](i64 i) {
    // There might be gaps between strings to satisfy alignment requirements.
    // If that's the case, we need to zero-clear them.
    if (this->shdr.sh_addralign > 1)
      memset(buf + shard_offsets[i], 0, shard_offsets[i + 1] - shard_offsets[i]);

    // Copy strings
    for (i64 j = shard_size * i; j < shard_size * (i + 1); j++)
      if (const char *key = map.entries[j].key)
        if (SectionFragment<E> &frag = map.entries[j].value; frag.is_alive)
          memcpy(buf + frag.offset, key, map.entries[j].keylen);
  });
}

template <typename E>
void MergedSection<E>::print_stats(Context<E> &ctx) {
  i64 used = 0;
  for (i64 i = 0; i < map.nbuckets; i++)
    if (map.entries[i].key)
      used++;

  SyncOut(ctx) << this->name
               << " estimation=" << estimator.get_cardinality()
               << " actual=" << used;
}

template <typename E>
void EhFrameSection<E>::construct(Context<E> &ctx) {
  Timer t(ctx, "eh_frame");

  // If .eh_frame is missing in all input files, we don't want to
  // create an output .eh_frame section.
  if (std::all_of(ctx.objs.begin(), ctx.objs.end(),
                  [](ObjectFile<E> *file) { return file->cies.empty(); })) {
    this->shdr.sh_size = 0;
    return;
  }

  // Remove dead FDEs and assign them offsets within their corresponding
  // CIE group.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    std::erase_if(file->fdes, [](FdeRecord<E> &fde) { return !fde.is_alive; });

    i64 offset = 0;
    for (FdeRecord<E> &fde : file->fdes) {
      fde.output_offset = offset;
      offset += fde.size(*file);
    }
    file->fde_size = offset;
  });

  // Uniquify CIEs and assign offsets to them.
  std::vector<CieRecord<E> *> leaders;
  auto find_leader = [&](CieRecord<E> &cie) -> CieRecord<E> * {
    for (CieRecord<E> *leader : leaders)
      if (cie_equals(*leader, cie))
        return leader;
    return nullptr;
  };

  i64 offset = 0;
  for (ObjectFile<E> *file : ctx.objs) {
    for (CieRecord<E> &cie : file->cies) {
      if (CieRecord<E> *leader = find_leader(cie)) {
        cie.output_offset = leader->output_offset;
      } else {
        cie.output_offset = offset;
        cie.is_leader = true;
        offset += cie.size();
        leaders.push_back(&cie);
      }
    }
  }

  // Assign FDE offsets to files.
  i64 idx = 0;
  for (ObjectFile<E> *file : ctx.objs) {
    file->fde_idx = idx;
    idx += file->fdes.size();

    file->fde_offset = offset;
    offset += file->fde_size;
  }

  // .eh_frame must end with a null word.
  this->shdr.sh_size = offset + 4;
}

// Write to .eh_frame and .eh_frame_hdr.
template <typename E>
void EhFrameSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;

  struct HdrEntry {
    I32<E> init_addr;
    I32<E> fde_addr;
  };

  HdrEntry *eh_hdr = nullptr;
  if (ctx.eh_frame_hdr)
    eh_hdr = (HdrEntry *)(ctx.buf + ctx.eh_frame_hdr->shdr.sh_offset +
                   EhFrameHdrSection<E>::HEADER_SIZE);

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    // Copy CIEs.
    for (CieRecord<E> &cie : file->cies) {
      if (!cie.is_leader)
        continue;

      std::string_view contents = cie.get_contents();
      memcpy(base + cie.output_offset, contents.data(), contents.size());

      if (ctx.arg.relocatable)
        continue;

      for (const ElfRel<E> &rel : cie.get_rels()) {
        assert(rel.r_offset - cie.input_offset < contents.size());

        Symbol<E> &sym = *file->symbols[rel.r_sym];
        u64 loc = cie.output_offset + rel.r_offset - cie.input_offset;
        u64 val = sym.get_addr(ctx) + get_addend(cie.input_section, rel);
        apply_eh_reloc(ctx, rel, loc, val);
      }
    }

    // Copy FDEs.
    for (i64 i = 0; i < file->fdes.size(); i++) {
      FdeRecord<E> &fde = file->fdes[i];
      std::span<ElfRel<E>> rels = fde.get_rels(*file);
      i64 offset = file->fde_offset + fde.output_offset;

      std::string_view contents = fde.get_contents(*file);
      memcpy(base + offset, contents.data(), contents.size());

      CieRecord<E> &cie = file->cies[fde.cie_idx];
      *(U32<E> *)(base + offset + 4) = offset + 4 - cie.output_offset;

      if (ctx.arg.relocatable)
        continue;

      for (const ElfRel<E> &rel : rels) {
        assert(rel.r_offset - fde.input_offset < contents.size());

        Symbol<E> &sym = *file->symbols[rel.r_sym];
        u64 loc = offset + rel.r_offset - fde.input_offset;
        u64 val = sym.get_addr(ctx) + get_addend(cie.input_section, rel);
        apply_eh_reloc(ctx, rel, loc, val);
      }

      if (eh_hdr) {
        // Write to .eh_frame_hdr
        Symbol<E> &sym = *file->symbols[rels[0].r_sym];
        u64 val = sym.get_addr(ctx) + get_addend(cie.input_section, rels[0]);
        u64 sh_addr = ctx.eh_frame_hdr->shdr.sh_addr;

        HdrEntry &ent = eh_hdr[file->fde_idx + i];
        ent.init_addr = val - sh_addr;
        ent.fde_addr = this->shdr.sh_addr + offset - sh_addr;
      }
    }
  });

  // Write a terminator.
  *(U32<E> *)(base + this->shdr.sh_size - 4) = 0;

  // Sort .eh_frame_hdr contents.
  if (eh_hdr) {
    tbb::parallel_sort(eh_hdr, eh_hdr + ctx.eh_frame_hdr->num_fdes,
                      [](const HdrEntry &a, const HdrEntry &b) {
      return a.init_addr < b.init_addr;
    });
  }
}

template <typename E>
void EhFrameHdrSection<E>::update_shdr(Context<E> &ctx) {
  num_fdes = 0;
  for (ObjectFile<E> *file : ctx.objs)
    num_fdes += file->fdes.size();
  this->shdr.sh_size = HEADER_SIZE + num_fdes * 8;
}

template <typename E>
void EhFrameHdrSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;

  // Write a header. The actual table is written by EhFrameHdr<E>::copy_buf.
  base[0] = 1;
  base[1] = DW_EH_PE_pcrel | DW_EH_PE_sdata4;
  base[2] = DW_EH_PE_udata4;
  base[3] = DW_EH_PE_datarel | DW_EH_PE_sdata4;

  *(U32<E> *)(base + 4) = ctx.eh_frame->shdr.sh_addr - this->shdr.sh_addr - 4;
  *(U32<E> *)(base + 8) = num_fdes;
}

template <typename E>
void EhFrameRelocSection<E>::update_shdr(Context<E> &ctx) {
  tbb::enumerable_thread_specific<i64> count;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (CieRecord<E> &cie : file->cies)
      if (cie.is_leader)
        count.local() += cie.get_rels().size();

    for (FdeRecord<E> &fde : file->fdes)
      count.local() += fde.get_rels(*file).size();
  });

  this->shdr.sh_size = count.combine(std::plus()) * sizeof(ElfRel<E>);
  this->shdr.sh_link = ctx.symtab->shndx;
  this->shdr.sh_info = ctx.eh_frame->shndx;
}

template <typename E>
void EhFrameRelocSection<E>::copy_buf(Context<E> &ctx) {
  ElfRel<E> *buf = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset);

  auto copy = [&](ObjectFile<E> &file, InputSection<E> &isec,
                  const ElfRel<E> &r, u64 offset) {
    Symbol<E> &sym = *file.symbols[r.r_sym];
    memset(buf, 0, sizeof(*buf));

    if (sym.esym().st_type == STT_SECTION) {
      // We discard section symbols in input files and re-create new
      // ones for each output section. So we need to adjust relocations'
      // addends if they refer a section symbol.
      InputSection<E> *target = sym.get_input_section();
      buf->r_sym = target->output_section->shndx;

      if constexpr (E::is_rela)
        buf->r_addend = get_addend(isec, r) + target->offset;
      else if (ctx.arg.relocatable)
        write_addend(ctx.buf + ctx.eh_frame->shdr.sh_offset + offset,
                     get_addend(isec, r) + target->offset, r);
    } else {
      buf->r_sym = sym.get_output_sym_idx(ctx);
      if constexpr (E::is_rela)
        buf->r_addend = get_addend(isec, r);
    }

    buf->r_offset = ctx.eh_frame->shdr.sh_addr + offset;
    buf->r_type = r.r_type;
    buf++;
  };

  for (ObjectFile<E> *file : ctx.objs) {
    for (CieRecord<E> &cie : file->cies)
      if (cie.is_leader)
        for (const ElfRel<E> &rel : cie.get_rels())
          copy(*file, cie.input_section, rel,
               cie.output_offset + rel.r_offset - cie.input_offset);

    for (FdeRecord<E> &fde : file->fdes) {
      i64 offset = file->fde_offset + fde.output_offset;
      for (const ElfRel<E> &rel : fde.get_rels(*file))
        copy(*file, file->cies[fde.cie_idx].input_section, rel,
             offset + rel.r_offset - fde.input_offset);
    }
  }
}

template <typename E>
void CopyrelSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  if (sym->has_copyrel)
    return;

  assert(!ctx.arg.shared);
  assert(sym->file->is_dso);

  symbols.push_back(sym);

  SharedFile<E> &file = *(SharedFile<E> *)sym->file;
  i64 alignment = file.get_alignment(sym);
  u64 offset = align_to(this->shdr.sh_size, alignment);

  this->shdr.sh_size = offset + sym->esym().st_size;
  this->shdr.sh_addralign = std::max<i64>(alignment, this->shdr.sh_addralign);

  // We need to create dynamic symbols not only for this particular symbol
  // but also for its aliases (i.e. other symbols at the same address)
  // becasue otherwise the aliases are broken apart at runtime.
  // For example, `environ`, `_environ` and `__environ` in libc.so are
  // aliases. If one of the symbols is copied by a copy relocation, other
  // symbols have to refer to the copied place as well.
  for (Symbol<E> *sym2 : file.get_symbols_at(sym)) {
    sym2->add_aux(ctx);
    sym2->is_imported = true;
    sym2->is_exported = true;
    sym2->has_copyrel = true;
    sym2->is_copyrel_readonly = this->is_relro;
    sym2->value = offset;
    ctx.dynsym->add_symbol(ctx, sym2);
  }
}

template <typename E>
void CopyrelSection<E>::copy_buf(Context<E> &ctx) {
  ElfRel<E> *rel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                                 this->reldyn_offset);

  for (Symbol<E> *sym : symbols)
    *rel++ = ElfRel<E>(sym->get_addr(ctx), E::R_COPY,
                       sym->get_dynsym_idx(ctx), 0);
}

template <typename E>
void VersymSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = contents.size() * sizeof(contents[0]);
  this->shdr.sh_link = ctx.dynsym->shndx;
}

template <typename E>
void VersymSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->shdr.sh_offset, contents);
}

// If `-z pack-relative-relocs` is specified, we'll create a .relr.dyn
// section and store base relocation records to that section instead of
// to the usual .rela.dyn section.
//
// .relr.dyn is relatively new feature and not supported by glibc until
// 2.38 which was released in 2022. If we don't do anything, executables
// built with `-z pack-relative-relocs` 't work and would crash
// immediately on startup with an older version of glibc.
//
// As a workaround, we'll add a dependency to a dummy version name
// "GLIBC_ABI_DT_RELR" if `-z pack-relative-relocs` is given so that
// executables built with the option failed with a more friendly "version
// `GLIBC_ABI_DT_RELR' not found" error message. glibc 2.38 or later knows
// about this dummy version name and simply ignores it.
template <typename E>
static InputFile<E> *find_glibc2(Context<E> &ctx) {
  for (Symbol<E> *sym : ctx.dynsym->symbols)
    if (sym && sym->file->is_dso &&
        ((SharedFile<E> *)sym->file)->soname.starts_with("libc.so.") &&
        sym->get_version().starts_with("GLIBC_2."))
      return sym->file;
  return nullptr;
}

template <typename E>
void VerneedSection<E>::construct(Context<E> &ctx) {
  Timer t(ctx, "fill_verneed");

  // Create a list of versioned symbols and sort by file and version.
  std::vector<Symbol<E> *> syms;

  for (i64 i = 1; i < ctx.dynsym->symbols.size(); i++) {
    Symbol<E> &sym = *ctx.dynsym->symbols[i];
    if (sym.file->is_dso && VER_NDX_LAST_RESERVED < sym.ver_idx)
      syms.push_back(&sym);
  }

  if (syms.empty())
    return;

  sort(syms, [](Symbol<E> *a, Symbol<E> *b) {
    return std::tuple(((SharedFile<E> *)a->file)->soname, a->ver_idx) <
           std::tuple(((SharedFile<E> *)b->file)->soname, b->ver_idx);
  });

  // Resize .gnu.version
  ctx.versym->contents.resize(ctx.dynsym->symbols.size(), VER_NDX_GLOBAL);
  ctx.versym->contents[0] = VER_NDX_LOCAL;

  // Allocate a large enough buffer for .gnu.version_r.
  contents.resize((sizeof(ElfVerneed<E>) + sizeof(ElfVernaux<E>)) *
                  (syms.size() + 1));

  // Fill .gnu.version_r.
  u8 *buf = (u8 *)&contents[0];
  u8 *ptr = buf;
  ElfVerneed<E> *verneed = nullptr;
  ElfVernaux<E> *aux = nullptr;

  i64 veridx = VER_NDX_LAST_RESERVED + ctx.arg.version_definitions.size();

  auto start_group = [&](InputFile<E> *file) {
    this->shdr.sh_info++;
    if (verneed)
      verneed->vn_next = ptr - (u8 *)verneed;

    verneed = (ElfVerneed<E> *)ptr;
    ptr += sizeof(ElfVerneed<E>);
    verneed->vn_version = 1;
    verneed->vn_file = ctx.dynstr->find_string(((SharedFile<E> *)file)->soname);
    verneed->vn_aux = sizeof(ElfVerneed<E>);
    aux = nullptr;
  };

  auto add_entry = [&](std::string_view verstr) {
    verneed->vn_cnt++;

    if (aux)
      aux->vna_next = sizeof(ElfVernaux<E>);
    aux = (ElfVernaux<E> *)ptr;
    ptr += sizeof(ElfVernaux<E>);

    aux->vna_hash = elf_hash(verstr);
    aux->vna_other = ++veridx;
    aux->vna_name = ctx.dynstr->add_string(verstr);
  };

  // Create version entries.
  for (i64 i = 0; i < syms.size(); i++) {
    if (i == 0 || syms[i - 1]->file != syms[i]->file) {
      start_group(syms[i]->file);
      add_entry(syms[i]->get_version());
    } else if (syms[i - 1]->ver_idx != syms[i]->ver_idx) {
      add_entry(syms[i]->get_version());
    }

    ctx.versym->contents[syms[i]->get_dynsym_idx(ctx)] = veridx;
  }

  if (ctx.arg.pack_dyn_relocs_relr) {
    if (InputFile<E> *file = find_glibc2(ctx)) {
      start_group(file);
      add_entry("GLIBC_ABI_DT_RELR");
    }
  }

  // Resize .gnu.version_r to fit to its contents.
  contents.resize(ptr - buf);
}

template <typename E>
void VerneedSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = contents.size();
  this->shdr.sh_link = ctx.dynstr->shndx;
}

template <typename E>
void VerneedSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->shdr.sh_offset, contents);
}

template <typename E>
void VerdefSection<E>::construct(Context<E> &ctx) {
  Timer t(ctx, "fill_verdef");

  if (ctx.arg.version_definitions.empty())
    return;

  // Resize .gnu.version
  ctx.versym->contents.resize(ctx.dynsym->symbols.size(), VER_NDX_GLOBAL);
  ctx.versym->contents[0] = VER_NDX_LOCAL;

  // Allocate a buffer for .gnu.version_d.
  contents.resize((sizeof(ElfVerdef<E>) + sizeof(ElfVerdaux<E>)) *
                  (ctx.arg.version_definitions.size() + 1));

  u8 *buf = (u8 *)&contents[0];
  u8 *ptr = buf;
  ElfVerdef<E> *verdef = nullptr;

  auto write = [&](std::string_view verstr, i64 idx, i64 flags) {
    this->shdr.sh_info++;
    if (verdef)
      verdef->vd_next = ptr - (u8 *)verdef;

    verdef = (ElfVerdef<E> *)ptr;
    ptr += sizeof(ElfVerdef<E>);

    verdef->vd_version = 1;
    verdef->vd_flags = flags;
    verdef->vd_ndx = idx;
    verdef->vd_cnt = 1;
    verdef->vd_hash = elf_hash(verstr);
    verdef->vd_aux = sizeof(ElfVerdef<E>);

    ElfVerdaux<E> *aux = (ElfVerdaux<E> *)ptr;
    ptr += sizeof(ElfVerdaux<E>);
    aux->vda_name = ctx.dynstr->add_string(verstr);
  };

  if (!ctx.arg.soname.empty())
    write(ctx.arg.soname, 1, VER_FLG_BASE);
  else
    write(ctx.arg.output, 1, VER_FLG_BASE);

  i64 idx = 2;
  for (std::string_view verstr : ctx.arg.version_definitions)
    write(verstr, idx++, 0);

  for (i64 i = 1; i < ctx.dynsym->symbols.size(); i++)
    if (Symbol<E> &sym = *ctx.dynsym->symbols[i];
        !sym.file->is_dso && sym.ver_idx != VER_NDX_UNSPECIFIED)
      ctx.versym->contents[sym.get_dynsym_idx(ctx)] = sym.ver_idx;
}

template <typename E>
void VerdefSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = contents.size();
  this->shdr.sh_link = ctx.dynstr->shndx;
}

template <typename E>
void VerdefSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->shdr.sh_offset, contents);
}

inline i64 BuildId::size() const {
  switch (kind) {
  case HEX:
    return value.size();
  case HASH:
    return hash_size;
  case UUID:
    return 16;
  default:
    unreachable();
  }
}

template <typename E>
void BuildIdSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = HEADER_SIZE + ctx.arg.build_id.size();
}

template <typename E>
void BuildIdSection<E>::copy_buf(Context<E> &ctx) {
  U32<E> *base = (U32<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(base, 0, this->shdr.sh_size);
  base[0] = 4;                          // Name size
  base[1] = ctx.arg.build_id.size();    // Hash size
  base[2] = NT_GNU_BUILD_ID;            // Type
  memcpy(base + 3, "GNU", 4);           // Name string
}

// BLAKE3 is a cryptographic hash function just like SHA256.
// We use it instead of SHA256 because it's faster.
static void blake3_hash(u8 *buf, i64 size, u8 *out) {
  blake3_hasher hasher;
  blake3_hasher_init(&hasher);
  blake3_hasher_update(&hasher, buf, size);
  blake3_hasher_finalize(&hasher, out, BLAKE3_OUT_LEN);
}

template <typename E>
static void compute_blake3(Context<E> &ctx, u8 *buf) {
  i64 shard_size = 4 * 1024 * 1024;
  i64 filesize = ctx.output_file->filesize;
  i64 num_shards = align_to(filesize, shard_size) / shard_size;
  std::vector<u8> shards(num_shards * BLAKE3_OUT_LEN);

  tbb::parallel_for((i64)0, num_shards, [&](i64 i) {
    u8 *begin = ctx.buf + shard_size * i;
    u8 *end = (i == num_shards - 1) ? ctx.buf + filesize : begin + shard_size;
    blake3_hash(begin, end - begin, shards.data() + i * BLAKE3_OUT_LEN);

#ifdef HAVE_MADVISE
    // Make the kernel page out the file contents we've just written
    // so that subsequent close(2) call will become quicker.
    if (i > 0 && ctx.output_file->is_mmapped)
      madvise(begin, end - begin, MADV_DONTNEED);
#endif
   });

  u8 digest[BLAKE3_OUT_LEN];
  blake3_hash(shards.data(), shards.size(), digest);

  assert(ctx.arg.build_id.size() <= BLAKE3_OUT_LEN);
  memcpy(buf, digest, ctx.arg.build_id.size());
}

template <typename E>
void BuildIdSection<E>::write_buildid(Context<E> &ctx) {
  Timer t(ctx, "build_id");
  u8 *buf = ctx.buf + this->shdr.sh_offset + HEADER_SIZE;

  switch (ctx.arg.build_id.kind) {
  case BuildId::HEX:
    write_vector(buf, ctx.arg.build_id.value);
    return;
  case BuildId::HASH:
    compute_blake3(ctx, buf);
    return;
  case BuildId::UUID:
    write_uuid_v4(buf);
    return;
  default:
    unreachable();
  }
}

template <typename E>
void NotePackageSection<E>::update_shdr(Context<E> &ctx) {
  if (!ctx.arg.package_metadata.empty()) {
    // +17 is for the header and the NUL terminator
    this->shdr.sh_size = align_to(ctx.arg.package_metadata.size() + 17, 4);
  }
}

template <typename E>
void NotePackageSection<E>::copy_buf(Context<E> &ctx) {
  U32<E> *buf = (U32<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  buf[0] = 4;                                      // Name size
  buf[1] = this->shdr.sh_size - 16;                // Content size
  buf[2] = NT_FDO_PACKAGING_METADATA;              // Type
  memcpy(buf + 3, "FDO", 4);                       // Name
  write_string(buf + 4, ctx.arg.package_metadata); // Content
}

// Merges input files' .note.gnu.property values.
template <typename E>
void NotePropertySection<E>::update_shdr(Context<E> &ctx) {
  // The rules we support are only specified for x86 psABI
  if (!is_x86<E>)
    return;

  // Reset to the initial state so that this function is idempotent
  properties.clear();

  // Obtain the list of keys
  std::vector<ObjectFile<E> *> files = ctx.objs;
  std::erase(files, ctx.internal_obj);
  std::set<u32> keys;

  for (ObjectFile<E> *file : files)
    for (std::pair<u32, u32> kv : file->gnu_properties)
      keys.insert(kv.first);

  auto get_value = [](ObjectFile<E> *file, u32 key) -> u32 {
    auto it = file->gnu_properties.find(key);
    if (it != file->gnu_properties.end())
      return it->second;
    return 0;
  };

  // Merge values for each key
  for (u32 key : keys) {
    auto has_key = [&](ObjectFile<E> *file) {
      return file->gnu_properties.contains(key);
    };

    if (GNU_PROPERTY_X86_UINT32_AND_LO <= key &&
        key <= GNU_PROPERTY_X86_UINT32_AND_HI) {
      // An AND feature is set if all input objects have the property and
      // the feature.
      if (std::all_of(files.begin(), files.end(), has_key)) {
        properties[key] = 0xffff'ffff;
        for (ObjectFile<E> *file : files)
          properties[key] &= get_value(file, key);
      }
    } else if (GNU_PROPERTY_X86_UINT32_OR_LO <= key &&
               key <= GNU_PROPERTY_X86_UINT32_OR_HI) {
      // An OR feature is set if some input object has the feature.
      for (ObjectFile<E> *file : files)
        properties[key] |= get_value(file, key);
    } else if (GNU_PROPERTY_X86_UINT32_OR_AND_LO <= key &&
               key <= GNU_PROPERTY_X86_UINT32_OR_AND_HI) {
      // An OR-AND feature is set if all input object files have the property
      // and some of them has the feature.
      if (std::all_of(files.begin(), files.end(), has_key))
        for (ObjectFile<E> *file : files)
          properties[key] |= get_value(file, key);
    }
  }

  if (ctx.arg.z_ibt)
    properties[GNU_PROPERTY_X86_FEATURE_1_AND] |= GNU_PROPERTY_X86_FEATURE_1_IBT;
  if (ctx.arg.z_shstk)
    properties[GNU_PROPERTY_X86_FEATURE_1_AND] |= GNU_PROPERTY_X86_FEATURE_1_SHSTK;

  std::erase_if(properties, [](std::pair<u32, u32> kv) {
    return kv.second == 0;
  });

  if (properties.empty())
    this->shdr.sh_size = 0;
  else
    this->shdr.sh_size = 16 + ENTRY_SIZE * properties.size();
}

template <typename E>
void NotePropertySection<E>::copy_buf(Context<E> &ctx) {
  U32<E> *buf = (U32<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  buf[0] = 4;                              // Name size
  buf[1] = ENTRY_SIZE * properties.size(); // Content size
  buf[2] = NT_GNU_PROPERTY_TYPE_0;         // Type
  memcpy(buf + 3, "GNU", 4);               // Name

  i64 idx = 4;
  for (std::pair<u32, u32> kv : properties) {
    buf[idx] = kv.first;                   // Feature type
    buf[idx + 1] = 4;                      // Feature size
    buf[idx + 2] = kv.second;              // Feature flags
    idx += ENTRY_SIZE / sizeof(U32<E>);
  }
}

template <typename E>
CompressedSection<E>::CompressedSection(Context<E> &ctx, Chunk<E> &chunk) {
  assert(chunk.name.starts_with(".debug"));
  this->name = chunk.name;
  this->is_compressed = true;

  this->uncompressed_data.resize(chunk.shdr.sh_size);
  u8 *buf = this->uncompressed_data.data();

  chunk.write_to(ctx, buf);

  switch (ctx.arg.compress_debug_sections) {
  case COMPRESS_ZLIB:
    chdr.ch_type = ELFCOMPRESS_ZLIB;
    compressor.reset(new ZlibCompressor(buf, chunk.shdr.sh_size));
    break;
  case COMPRESS_ZSTD:
    chdr.ch_type = ELFCOMPRESS_ZSTD;
    compressor.reset(new ZstdCompressor(buf, chunk.shdr.sh_size));
    break;
  default:
    unreachable();
  }

  chdr.ch_size = chunk.shdr.sh_size;
  chdr.ch_addralign = chunk.shdr.sh_addralign;

  this->shdr = chunk.shdr;
  this->shdr.sh_flags |= SHF_COMPRESSED;
  this->shdr.sh_addralign = 1;
  this->shdr.sh_size = sizeof(chdr) + compressor->compressed_size;
  this->shndx = chunk.shndx;

  // We don't need to keep the original data unless --gdb-index is given.
  if (!ctx.arg.gdb_index) {
    this->uncompressed_data.clear();
    this->uncompressed_data.shrink_to_fit();
  }
}

template <typename E>
void CompressedSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memcpy(base, &chdr, sizeof(chdr));
  compressor->write_to(base + sizeof(chdr));
}

template <typename E>
RelocSection<E>::RelocSection(Context<E> &ctx, OutputSection<E> &osec)
  : output_section(osec) {
  if constexpr (E::is_rela) {
    this->name = save_string(ctx, ".rela" + std::string(osec.name));
    this->shdr.sh_type = SHT_RELA;
  } else {
    this->name = save_string(ctx, ".rel" + std::string(osec.name));
    this->shdr.sh_type = SHT_REL;
  }

  this->shdr.sh_flags = SHF_INFO_LINK;
  this->shdr.sh_addralign = sizeof(Word<E>);
  this->shdr.sh_entsize = sizeof(ElfRel<E>);

  // Compute an offset for each input section
  offsets.resize(osec.members.size());

  auto scan = [&](const tbb::blocked_range<i64> &r, i64 sum, bool is_final) {
    for (i64 i = r.begin(); i < r.end(); i++) {
      InputSection<E> &isec = *osec.members[i];
      if (is_final)
        offsets[i] = sum;
      sum += isec.get_rels(ctx).size();
    }
    return sum;
  };

  i64 num_entries = tbb::parallel_scan(
    tbb::blocked_range<i64>(0, osec.members.size()), 0, scan, std::plus());

  this->shdr.sh_size = num_entries * sizeof(ElfRel<E>);
}

template <typename E>
void RelocSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_link = ctx.symtab->shndx;
  this->shdr.sh_info = output_section.shndx;
}

template <typename E>
void RelocSection<E>::copy_buf(Context<E> &ctx) {
  auto write = [&](ElfRel<E> &out, InputSection<E> &isec, const ElfRel<E> &rel) {
    Symbol<E> &sym = *isec.file.symbols[rel.r_sym];
    i64 symidx = 0;
    i64 addend = 0;

    if (sym.esym().st_type == STT_SECTION) {
      if (SectionFragment<E> *frag = sym.get_frag()) {
        symidx = frag->output_section.shndx;
        addend = frag->offset + sym.value + get_addend(isec, rel);
      } else {
        InputSection<E> *target = sym.get_input_section();

        if (OutputSection<E> *osec = target->output_section) {
          symidx = osec->shndx;
          addend = get_addend(isec, rel) + target->offset;
        } else if (isec.name() == ".eh_frame") {
          symidx = ctx.eh_frame->shndx;
          addend = get_addend(isec, rel);
        } else {
          // This is usually a dead debug section referring a
          // COMDAT-eliminated section.
        }
      }
    } else if (sym.write_to_symtab) {
      symidx = sym.get_output_sym_idx(ctx);
      addend = get_addend(isec, rel);
    }

    if constexpr (is_alpha<E>)
      if (rel.r_type == R_ALPHA_GPDISP || rel.r_type == R_ALPHA_LITUSE)
        addend = rel.r_addend;

    i64 r_offset = isec.output_section->shdr.sh_addr + isec.offset + rel.r_offset;
    out = ElfRel<E>(r_offset, rel.r_type, symidx, addend);

    if (ctx.arg.relocatable) {
      u8 *base = ctx.buf + isec.output_section->shdr.sh_offset + isec.offset;
      write_addend(base + rel.r_offset, addend, rel);
    }
  };

  tbb::parallel_for((i64)0, (i64)output_section.members.size(), [&](i64 i) {
    ElfRel<E> *buf = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset) + offsets[i];
    InputSection<E> &isec = *output_section.members[i];
    std::span<const ElfRel<E>> rels = isec.get_rels(ctx);

    for (i64 j = 0; j < rels.size(); j++)
      write(buf[j], isec, rels[j]);
  });
}

template <typename E>
void ComdatGroupSection<E>::update_shdr(Context<E> &ctx) {
  assert(ctx.arg.relocatable);
  this->shdr.sh_link = ctx.symtab->shndx;

  if (sym.esym().st_type == STT_SECTION)
    this->shdr.sh_info = sym.get_input_section()->output_section->shndx;
  else
    this->shdr.sh_info = sym.get_output_sym_idx(ctx);
}

template <typename E>
void ComdatGroupSection<E>::copy_buf(Context<E> &ctx) {
  U32<E> *buf = (U32<E> *)(ctx.buf + this->shdr.sh_offset);
  *buf++ = GRP_COMDAT;
  for (Chunk<E> *chunk : members)
    *buf++ = chunk->shndx;
}

using E = MOLD_TARGET;

template class Chunk<E>;
template class OutputEhdr<E>;
template class OutputShdr<E>;
template class OutputPhdr<E>;
template class InterpSection<E>;
template class OutputSection<E>;
template class GotSection<E>;
template class GotPltSection<E>;
template class PltSection<E>;
template class PltGotSection<E>;
template class RelPltSection<E>;
template class RelDynSection<E>;
template class RelrDynSection<E>;
template class StrtabSection<E>;
template class ShstrtabSection<E>;
template class DynstrSection<E>;
template class DynamicSection<E>;
template class SymtabSection<E>;
template class DynsymSection<E>;
template class HashSection<E>;
template class GnuHashSection<E>;
template class MergedSection<E>;
template class EhFrameSection<E>;
template class EhFrameHdrSection<E>;
template class EhFrameRelocSection<E>;
template class CopyrelSection<E>;
template class VersymSection<E>;
template class VerneedSection<E>;
template class VerdefSection<E>;
template class BuildIdSection<E>;
template class NotePackageSection<E>;
template class NotePropertySection<E>;
template class GdbIndexSection<E>;
template class CompressedSection<E>;
template class RelocSection<E>;
template class ComdatGroupSection<E>;
template i64 to_phdr_flags(Context<E> &ctx, Chunk<E> *chunk);
template ElfSym<E> to_output_esym(Context<E> &, Symbol<E> &, u32, U32<E> *);

} // namespace mold::elf
