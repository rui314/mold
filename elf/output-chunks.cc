#include "mold.h"
#include "../sha.h"

#include <cctype>
#include <shared_mutex>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_scan.h>
#include <tbb/parallel_sort.h>

#ifndef _WIN32
# include <sys/mman.h>
#endif

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
u64 get_entry_addr(Context<E> &ctx) {
  if (!ctx.arg.entry.empty())
    if (Symbol<E> *sym = get_symbol(ctx, ctx.arg.entry);
        sym->file && !sym->file->is_dso)
      return sym->get_addr(ctx);

  for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections)
    if (osec->name == ".text")
      return osec->shdr.sh_addr;
  return 0;
}

template <typename E>
u64 get_eflags(Context<E> &ctx) {
  if constexpr (std::is_same_v<E, ARM32>)
    return EF_ARM_EABI_VER5;

  if constexpr (is_riscv<E>) {
    std::vector<ObjectFile<E> *> objs = ctx.objs;
    std::erase(objs, ctx.internal_obj);

    if (objs.empty())
      return 0;

    u32 ret = objs[0]->get_ehdr().e_flags;
    for (i64 i = 1; i < objs.size(); i++)
      if (objs[i]->get_ehdr().e_flags & EF_RISCV_RVC)
        ret |= EF_RISCV_RVC;
    return ret;
  }

  if constexpr (std::is_same_v<E, PPC64V2>)
    return 2;
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
  hdr.e_type = ctx.arg.pic ? ET_DYN : ET_EXEC;
  hdr.e_machine = E::e_machine;
  hdr.e_version = EV_CURRENT;
  hdr.e_entry = get_entry_addr(ctx);
  hdr.e_phoff = ctx.phdr->shdr.sh_offset;
  hdr.e_shoff = ctx.shdr->shdr.sh_offset;
  hdr.e_flags = get_eflags(ctx);
  hdr.e_ehsize = sizeof(ElfEhdr<E>);
  hdr.e_phentsize = sizeof(ElfPhdr<E>);
  hdr.e_phnum = ctx.phdr->shdr.sh_size / sizeof(ElfPhdr<E>);
  hdr.e_shentsize = sizeof(ElfShdr<E>);
  hdr.e_shnum = ctx.shdr->shdr.sh_size / sizeof(ElfShdr<E>);
  hdr.e_shstrndx = ctx.shstrtab->shndx;
}

template <typename E>
void OutputShdr<E>::update_shdr(Context<E> &ctx) {
  i64 n = 0;
  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->shndx)
      n = chunk->shndx;
  this->shdr.sh_size = (n + 1) * sizeof(ElfShdr<E>);
}

template <typename E>
void OutputShdr<E>::copy_buf(Context<E> &ctx) {
  ElfShdr<E> *hdr = (ElfShdr<E> *)(ctx.buf + this->shdr.sh_offset);
  hdr[0] = {};

  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->shndx)
      hdr[chunk->shndx] = chunk->shdr;
}

template <typename E>
i64 to_phdr_flags(Context<E> &ctx, Chunk<E> *chunk) {
  if (ctx.arg.omagic)
    return PF_R | PF_W | PF_X;

  i64 ret = PF_R;
  bool write = (chunk->shdr.sh_flags & SHF_WRITE);
  if (write)
    ret |= PF_W;
  if ((!ctx.arg.rosegment && !write) ||
      (chunk->shdr.sh_flags & SHF_EXECINSTR))
    ret |= PF_X;
  return ret;
}

// PT_GNU_RELRO segment is a security mechanism to make more pages
// read-only than we could have done without it.
//
// Traditionally, sections are either read-only or read-write.  If a
// section contains dynamic relocations, it must have been put into a
// read-write segment so that the program loader can mutate its
// contents in memory, even if no one will write to it at runtime.
//
// RELRO segment allows us to make such pages writable only when a
// program is being loaded. After that, the page becomes read-only.
//
// Some sections, such as .init, .fini, .got, .dynamic, contain
// dynamic relocations but doesn't have to be writable at runtime,
// so they are put into a RELRO segment.
template <typename E>
bool is_relro(Context<E> &ctx, Chunk<E> *chunk) {
  u64 flags = chunk->shdr.sh_flags;
  u64 type = chunk->shdr.sh_type;

  if (flags & SHF_WRITE)
    return (flags & SHF_TLS) || type == SHT_INIT_ARRAY ||
           type == SHT_FINI_ARRAY || type == SHT_PREINIT_ARRAY ||
           chunk == ctx.got || chunk == ctx.dynamic ||
           chunk == ctx.relro_padding ||
           (ctx.arg.z_now && ctx.gotplt && chunk == ctx.gotplt) ||
           chunk->name == ".toc" || chunk->name.ends_with(".rel.ro");
  return false;
}

template <typename E>
static std::vector<ElfPhdr<E>> create_phdr(Context<E> &ctx) {
  std::vector<ElfPhdr<E>> vec;

  auto define = [&](u64 type, u64 flags, i64 min_align, Chunk<E> *chunk) {
    vec.push_back({});
    ElfPhdr<E> &phdr = vec.back();
    phdr.p_type = type;
    phdr.p_flags = flags;
    phdr.p_align = std::max<u64>(min_align, chunk->shdr.sh_addralign);
    phdr.p_offset = chunk->shdr.sh_offset;
    phdr.p_filesz =
      (chunk->shdr.sh_type == SHT_NOBITS) ? 0 : (u64)chunk->shdr.sh_size;
    phdr.p_vaddr = chunk->shdr.sh_addr;
    phdr.p_paddr = chunk->shdr.sh_addr;
    phdr.p_memsz = chunk->shdr.sh_size;
  };

  auto append = [&](Chunk<E> *chunk) {
    ElfPhdr<E> &phdr = vec.back();
    phdr.p_align = std::max<u64>(phdr.p_align, chunk->shdr.sh_addralign);
    if (!(chunk->shdr.sh_type == SHT_NOBITS))
      phdr.p_filesz = chunk->shdr.sh_addr + chunk->shdr.sh_size - phdr.p_vaddr;
    phdr.p_memsz = chunk->shdr.sh_addr + chunk->shdr.sh_size - phdr.p_vaddr;
  };

  auto is_bss = [](Chunk<E> *chunk) {
    return chunk->shdr.sh_type == SHT_NOBITS &&
           !(chunk->shdr.sh_flags & SHF_TLS);
  };

  auto is_tbss = [](Chunk<E> *chunk) {
    return chunk->shdr.sh_type == SHT_NOBITS &&
           (chunk->shdr.sh_flags & SHF_TLS);
  };

  auto is_note = [](Chunk<E> *chunk) {
    ElfShdr<E> &shdr = chunk->shdr;
    return (shdr.sh_type == SHT_NOTE) && (shdr.sh_flags & SHF_ALLOC);
  };

  // Create a PT_PHDR for the program header itself.
  if (ctx.phdr)
    define(PT_PHDR, PF_R, sizeof(Word<E>), ctx.phdr);

  // Create a PT_INTERP.
  if (ctx.interp)
    define(PT_INTERP, PF_R, 1, ctx.interp);

  // Create a PT_NOTE for SHF_NOTE sections.
  for (i64 i = 0, end = ctx.chunks.size(); i < end;) {
    Chunk<E> *first = ctx.chunks[i++];
    if (!is_note(first))
      continue;

    i64 flags = to_phdr_flags(ctx, first);
    i64 alignment = first->shdr.sh_addralign;
    define(PT_NOTE, flags, alignment, first);

    while (i < end && is_note(ctx.chunks[i]) &&
           to_phdr_flags(ctx, ctx.chunks[i]) == flags)
      append(ctx.chunks[i++]);
  }

  // Create PT_LOAD segments.
  {
    i64 idx = vec.size();
    std::vector<Chunk<E> *> chunks = ctx.chunks;
    std::erase_if(chunks, is_tbss);

    for (i64 i = 0, end = chunks.size(); i < end;) {
      Chunk<E> *first = chunks[i++];
      if (!(first->shdr.sh_flags & SHF_ALLOC))
        break;

      i64 flags = to_phdr_flags(ctx, first);
      define(PT_LOAD, flags, ctx.page_size, first);

      // Add contiguous ALLOC sections as long as they have the same
      // section flags and there's no on-disk gap in between.
      if (!is_bss(first))
        while (i < end && !is_bss(chunks[i]) &&
               to_phdr_flags(ctx, chunks[i]) == flags &&
               chunks[i]->shdr.sh_offset - first->shdr.sh_offset ==
               chunks[i]->shdr.sh_addr - first->shdr.sh_addr)
          append(chunks[i++]);

      while (i < end && is_bss(chunks[i]) &&
             to_phdr_flags(ctx, chunks[i]) == flags)
        append(chunks[i++]);
    }

    // The ELF spec says that "loadable segment entries in the program
    // header table appear in ascending order, sorted on the p_vaddr
    // member".
    std::stable_sort(vec.begin() + idx, vec.end(),
                     [](const ElfPhdr<E> &a, const ElfPhdr<E> &b) {
      return a.p_vaddr < b.p_vaddr;
    });
  }

  // Create a PT_TLS.
  for (i64 i = 0; i < ctx.chunks.size(); i++) {
    if (!(ctx.chunks[i]->shdr.sh_flags & SHF_TLS))
      continue;

    define(PT_TLS, PF_R, 1, ctx.chunks[i++]);
    while (i < ctx.chunks.size() && (ctx.chunks[i]->shdr.sh_flags & SHF_TLS))
      append(ctx.chunks[i++]);

    // Some types of TLS relocations are defined relative to the TLS
    // segment, so save its addresses for easy access.
    ElfPhdr<E> &phdr = vec.back();
    ctx.tls_begin = phdr.p_vaddr;

    // Each thread has its own value in TP (thread pointer) register, and
    // TLVs defined in the main executable are accessed relative to TP.
    //
    // On x86, SPARC and S390X, TP (%gs on i386, %fs on x86-64, %g7 on SPARC
    // and %a0/%a1 on S390X) refers past the end of all TLVs for historical
    // reasons. TLVs are accessed with negative offsets from TP.
    //
    // On ARM, the runtime appends two words at the beginning of TLV
    // template image when copying TLVs to per-thread area, so we need
    // to offset it.
    //
    // On PPC64, TP is 0x7000 (28 KiB) past the beginning of the TLV block
    // to maximize the addressable range for load/store instructions with
    // 16-bits signed immediates. It's not exactly 0x8000 (32 KiB) off
    // because there's a small implementation-defined piece of data before
    // the TLV block, and the runtime wants to access them efficiently
    // too.
    //
    // RISC-V just uses the beginning of the TLV block as TP. RISC-V
    // load/store instructions usually take 12-bits signed immediates,
    // so the beginning of TLV ± 2 KiB is accessible with a single
    // load/store instruction.
    if constexpr (is_x86<E> || is_sparc<E> || is_s390x<E>) {
      ctx.tp_addr = align_to(phdr.p_vaddr + phdr.p_memsz, phdr.p_align);
    } else if constexpr (is_arm<E>) {
      ctx.tp_addr = align_down(ctx.tls_begin - sizeof(Word<E>) * 2, phdr.p_align);
    } else if constexpr (is_ppc<E>) {
      ctx.tp_addr = ctx.tls_begin + 0x7000;
    } else {
      static_assert(is_riscv<E>);
      ctx.tp_addr = ctx.tls_begin;
    }
  }

  // Add PT_DYNAMIC
  if (ctx.dynamic && ctx.dynamic->shdr.sh_size)
    define(PT_DYNAMIC, PF_R | PF_W, 1, ctx.dynamic);

  // Add PT_GNU_EH_FRAME
  if (ctx.eh_frame_hdr)
    define(PT_GNU_EH_FRAME, PF_R, 1, ctx.eh_frame_hdr);

  // Add PT_GNU_STACK, which is a marker segment that doesn't really
  // contain any segments. It controls executable bit of stack area.
  ElfPhdr<E> phdr = {};
  phdr.p_type = PT_GNU_STACK,
  phdr.p_flags = ctx.arg.z_execstack ? (PF_R | PF_W | PF_X) : (PF_R | PF_W),
  phdr.p_align = 1;
  vec.push_back(phdr);

  // Create a PT_GNU_RELRO.
  if (ctx.arg.z_relro) {
    for (i64 i = 0; i < ctx.chunks.size(); i++) {
      if (!is_relro(ctx, ctx.chunks[i]))
        continue;

      define(PT_GNU_RELRO, PF_R, 1, ctx.chunks[i++]);
      while (i < ctx.chunks.size() && is_relro(ctx, ctx.chunks[i]))
        append(ctx.chunks[i++]);
      vec.back().p_align = 1;
    }
  }

  // Add PT_ARM_EDXIDX
  if constexpr (std::is_same_v<E, ARM32>) {
    for (Chunk<E> *chunk : ctx.chunks) {
      if (chunk->shdr.sh_type == SHT_ARM_EXIDX) {
        define(PT_ARM_EXIDX, PF_R, 4, chunk);
        break;
      }
    }
  }

  return vec;
}

template <typename E>
void OutputPhdr<E>::update_shdr(Context<E> &ctx) {
  phdrs = create_phdr(ctx);
  this->shdr.sh_size = phdrs.size() * sizeof(ElfPhdr<E>);
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
  this->shdr.sh_link = ctx.dynsym->shndx;

  // .rel.dyn contents are filled by GotSection::copy_buf(Context<E> &ctx) and
  // InputSection::apply_reloc_alloc().
  i64 offset = ctx.got->get_reldyn_size(ctx);

  offset += ctx.copyrel->symbols.size() * sizeof(ElfRel<E>);
  offset += ctx.copyrel_relro->symbols.size() * sizeof(ElfRel<E>);

  for (ObjectFile<E> *file : ctx.objs) {
    file->reldyn_offset = offset;
    offset += file->num_dynrel * sizeof(ElfRel<E>);
  }
  this->shdr.sh_size = offset;
}

template <typename E>
void RelDynSection<E>::copy_buf(Context<E> &ctx) {
  ElfRel<E> *rel = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset +
                                 ctx.got->get_reldyn_size(ctx));

  for (Symbol<E> *sym : ctx.copyrel->symbols)
    *rel++ = ElfRel<E>(sym->get_addr(ctx), E::R_COPY, sym->get_dynsym_idx(ctx), 0);

  for (Symbol<E> *sym : ctx.copyrel_relro->symbols)
    *rel++ = ElfRel<E>(sym->get_addr(ctx), E::R_COPY, sym->get_dynsym_idx(ctx), 0);
}

template <typename E>
void RelDynSection<E>::sort(Context<E> &ctx) {
  Timer t(ctx, "sort_dynamic_relocs");

  ElfRel<E> *begin = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset);
  ElfRel<E> *end = (ElfRel<E> *)((u8 *)begin + this->shdr.sh_size);

  auto get_rank = [](u32 r_type) {
    switch (r_type) {
    case E::R_RELATIVE: return 0;
    case E::R_IRELATIVE: return 2;
    default: return 1;
    }
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
  // We group IFUNC relocations at the end of .rel.dyn because we need to
  // mark them with `__rel_iplt_start and `__rel_iplt_end`.
  tbb::parallel_sort(begin, end, [&](const ElfRel<E> &a, const ElfRel<E> &b) {
    return std::tuple(get_rank(a.r_type), a.r_sym, a.r_offset) <
           std::tuple(get_rank(b.r_type), b.r_sym, b.r_offset);
  });
}

template <typename E>
void RelrDynSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_link = ctx.dynsym->shndx;

  i64 n = ctx.got->relr.size();
  for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections)
    n += osec->relr.size();
  this->shdr.sh_size = n * sizeof(Word<E>);
}

template <typename E>
void RelrDynSection<E>::copy_buf(Context<E> &ctx) {
  Word<E> *buf = (Word<E> *)(ctx.buf + this->shdr.sh_offset);

  for (u64 val : ctx.got->relr)
    *buf++ = (val & 1) ? val : (ctx.got->shdr.sh_addr + val);

  for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections)
    for (u64 val : osec->relr)
      *buf++ = (val & 1) ? val : (osec->shdr.sh_addr + val);
}

template <typename E>
void StrtabSection<E>::update_shdr(Context<E> &ctx) {
  i64 offset = 1;

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

  this->shdr.sh_size = offset;
}

template <typename E>
void ShstrtabSection<E>::update_shdr(Context<E> &ctx) {
  std::unordered_map<std::string_view, i64> map;
  i64 offset = 1;

  for (Chunk<E> *chunk : ctx.chunks) {
    if (!chunk->name.empty()) {
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
    if (!chunk->name.empty())
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

  for (std::pair<std::string_view, i64> pair : strings)
    write_string(base + pair.second, pair.first);

  if (!ctx.dynsym->symbols.empty()) {
    i64 offset = dynsym_offset;

    for (i64 i = 1; i < ctx.dynsym->symbols.size(); i++) {
      Symbol<E> &sym = *ctx.dynsym->symbols[i];
      write_string(base + offset, sym.name());
      offset += sym.name().size() + 1;
    }
  }
}

template <typename E>
void SymtabSection<E>::update_shdr(Context<E> &ctx) {
  i64 nsyms = 1;

  // Section symbols
  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->shndx && (chunk->shdr.sh_flags & SHF_ALLOC))
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

  // Write the initial NUL byte to .strtab.
  ctx.buf[ctx.strtab->shdr.sh_offset] = '\0';

  // Create section symbols
  for (Chunk<E> *chunk : ctx.chunks) {
    if (chunk->shndx && (chunk->shdr.sh_flags & SHF_ALLOC)) {
      ElfSym<E> &sym = buf[chunk->shndx];
      memset(&sym, 0, sizeof(sym));
      sym.st_type = STT_SECTION;
      sym.st_value = chunk->shdr.sh_addr;
      sym.st_shndx = chunk->shndx;
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
    define(is_rela<E> ? DT_RELA : DT_REL, ctx.reldyn->shdr.sh_addr);
    define(is_rela<E> ? DT_RELASZ : DT_RELSZ, ctx.reldyn->shdr.sh_size);
    define(is_rela<E> ? DT_RELAENT : DT_RELENT, sizeof(ElfRel<E>));
  }

  if (ctx.relrdyn) {
    define(DT_RELR, ctx.relrdyn->shdr.sh_addr);
    define(DT_RELRSZ, ctx.relrdyn->shdr.sh_size);
    define(DT_RELRENT, ctx.relrdyn->shdr.sh_entsize);
  }

  if (ctx.relplt->shdr.sh_size) {
    define(DT_JMPREL, ctx.relplt->shdr.sh_addr);
    define(DT_PLTRELSZ, ctx.relplt->shdr.sh_size);
    define(DT_PLTREL, is_rela<E> ? DT_RELA : DT_REL);
  }

  if constexpr (is_sparc<E>) {
    if (ctx.plt->shdr.sh_size)
      define(DT_PLTGOT, ctx.plt->shdr.sh_addr);
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

  if (ctx.__init_array_start->get_output_section()) {
    define(DT_INIT_ARRAY, ctx.__init_array_start->value);
    define(DT_INIT_ARRAYSZ,
           ctx.__init_array_end->value - ctx.__init_array_start->value);
  }

  if (ctx.__preinit_array_start->get_output_section()) {
    define(DT_PREINIT_ARRAY, ctx.__preinit_array_start->value);
    define(DT_PREINIT_ARRAYSZ,
           ctx.__preinit_array_end->value - ctx.__preinit_array_start->value);
  }

  if (ctx.__fini_array_start->get_output_section()) {
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

  if (Symbol<E> *sym = get_symbol(ctx, ctx.arg.init);
      sym->file && !sym->file->is_dso)
    define(DT_INIT, sym->get_addr(ctx));
  if (Symbol<E> *sym = get_symbol(ctx, ctx.arg.fini);
      sym->file && !sym->file->is_dso)
    define(DT_FINI, sym->get_addr(ctx));

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

  if (ctx.has_gottp_rel)
    flags |= DF_STATIC_TLS;
  if (ctx.has_textrel)
    flags |= DF_TEXTREL;

  if (flags)
    define(DT_FLAGS, flags);
  if (flags1)
    define(DT_FLAGS_1, flags1);

  if constexpr (is_ppc<E>) {
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
static std::string_view
get_output_name(Context<E> &ctx, std::string_view name, u64 flags) {
  if (ctx.arg.unique && ctx.arg.unique->match(name))
    return name;

  if ((name == ".rodata" || name.starts_with(".rodata.")) && (flags & SHF_MERGE))
    return (flags & SHF_STRINGS) ? ".rodata.str" : ".rodata.cst";
  if (name == ".ARM.exidx" || name.starts_with(".ARM.exidx."))
    return ".ARM.exidx";
  if (name == ".ARM.extab" || name.starts_with(".ARM.extab."))
    return ".ARM.extab";

  if (ctx.arg.z_keep_text_section_prefix) {
    static std::string_view prefixes[] = {
      ".text.hot.", ".text.unknown.", ".text.unlikely.", ".text.startup.",
      ".text.exit."
    };

    for (std::string_view prefix : prefixes) {
      std::string_view stem = prefix.substr(0, prefix.size() - 1);
      if (name == stem || name.starts_with(prefix))
        return stem;
    }
  }

  static std::string_view prefixes[] = {
    ".text.", ".data.rel.ro.", ".data.", ".rodata.", ".bss.rel.ro.", ".bss.",
    ".init_array.", ".fini_array.", ".tbss.", ".tdata.", ".gcc_except_table.",
    ".ctors.", ".dtors.", ".gnu.warning.",
  };

  for (std::string_view prefix : prefixes) {
    std::string_view stem = prefix.substr(0, prefix.size() - 1);
    if (name == stem || name.starts_with(prefix))
      return stem;
  }

  return name;
}

template <typename E>
static u64 canonicalize_type(std::string_view name, u64 type) {
  if (type == SHT_PROGBITS) {
    if (name == ".init_array" || name.starts_with(".init_array."))
      return SHT_INIT_ARRAY;
    if (name == ".fini_array" || name.starts_with(".fini_array."))
      return SHT_FINI_ARRAY;
  }

  if constexpr (std::is_same_v<E, X86_64>)
    if (type == SHT_X86_64_UNWIND)
      return SHT_PROGBITS;

  return type;
}

template <typename E>
OutputSection<E> *
OutputSection<E>::get_instance(Context<E> &ctx, std::string_view name,
                               u64 type, u64 flags) {
  name = get_output_name(ctx, name, flags);
  type = canonicalize_type<E>(name, type);
  flags = flags & ~(u64)SHF_GROUP & ~(u64)SHF_COMPRESSED & ~(u64)SHF_LINK_ORDER &
          ~(u64)SHF_GNU_RETAIN;

  // .init_array is usually writable. We don't want to create multiple
  // .init_array output sections, so make it always writable.
  // So is .fini_array.
  if (type == SHT_INIT_ARRAY || type == SHT_FINI_ARRAY)
    flags |= SHF_WRITE;

  auto find = [&]() -> OutputSection<E> * {
    for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections)
      if (name == osec->name && type == osec->shdr.sh_type &&
          flags == osec->shdr.sh_flags)
        return osec.get();
    return nullptr;
  };

  static std::shared_mutex mu;

  // Search for an exiting output section.
  {
    std::shared_lock lock(mu);
    if (OutputSection<E> *osec = find())
      return osec;
  }

  // Create a new output section.
  std::unique_lock lock(mu);
  if (OutputSection<E> *osec = find())
    return osec;

  OutputSection<E> *osec = new OutputSection(name, type, flags,
                                             ctx.output_sections.size());
  ctx.output_sections.emplace_back(osec);
  return osec;
}

template <typename E>
void OutputSection<E>::copy_buf(Context<E> &ctx) {
  if (this->shdr.sh_type != SHT_NOBITS)
    write_to(ctx, ctx.buf + this->shdr.sh_offset);
}

template <typename E>
void OutputSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  auto clear = [&](u8 *loc, i64 size) {
    // As a special case, .init and .fini are filled with NOPs because the
    // runtime executes the sections as if they were a single function.
    // .init and .fini are superceded by .init_array and .fini_array and
    // being actively used only on s390x though.
    if (is_s390x<E> && (this->name == ".init" || this->name == ".fini")) {
      for (i64 i = 0; i < size; i += 2)
        *(ub16 *)(loc + i) = 0x0700; // nop
    } else {
      memset(loc, 0, size);
    }
  };

  tbb::parallel_for((i64)0, (i64)members.size(), [&](i64 i) {
    // Copy section contents to an output file
    InputSection<E> &isec = *members[i];
    isec.write_to(ctx, buf + isec.offset);

    // Clear trailing padding
    u64 this_end = isec.offset + isec.sh_size;
    u64 next_start = (i == members.size() - 1) ?
      (u64)this->shdr.sh_size : members[i + 1]->offset;
    clear(buf + this_end, next_start - this_end);
  });

  if constexpr (needs_thunk<E>) {
    tbb::parallel_for_each(thunks,
                           [&](std::unique_ptr<RangeExtensionThunk<E>> &thunk) {
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
static std::vector<u64> encode_relr(std::span<u64> pos, i64 word_size) {
  std::vector<u64> vec;
  u64 num_bits = word_size * 8 - 1;
  u64 max_delta = num_bits * word_size;

  for (i64 i = 0; i < pos.size();) {
    assert(i == 0 || pos[i - 1] <= pos[i]);
    assert(pos[i] % word_size == 0);

    vec.push_back(pos[i]);
    u64 base = pos[i] + word_size;
    i++;

    for (;;) {
      u64 bits = 0;
      for (; i < pos.size() && pos[i] - base < max_delta; i++)
        bits |= 1LL << ((pos[i] - base) / word_size);

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
    if ((1 << isec.p2align) < sizeof(Word<E>))
      return;

    for (const ElfRel<E> &r : isec.get_rels(ctx))
      if (r.r_type == E::R_ABS && (r.r_offset % sizeof(Word<E>)) == 0)
        if (Symbol<E> &sym = *isec.file.symbols[r.r_sym];
            !sym.is_absolute() && !sym.is_imported)
          shards[i].push_back(isec.offset + r.r_offset);
  });

  // Compress them
  std::vector<u64> pos = flatten(shards);
  relr = encode_relr(pos, sizeof(Word<E>));
}

// Compute spaces needed for thunk symbols
template <typename E>
void OutputSection<E>::compute_symtab_size(Context<E> &ctx) {
  if (ctx.arg.strip_all || ctx.arg.retain_symbols_file)
    return;

  if constexpr (needs_thunk<E>) {
    this->strtab_size = 0;
    this->num_local_symtab = 0;

    if constexpr (std::is_same_v<E, ARM32>)
      this->strtab_size = 9; // for "$t", "$a" and "$d" symbols

    for (std::unique_ptr<RangeExtensionThunk<E>> &thunk : thunks) {
      // For ARM32, we emit additional symbol "$t", "$a" and "$d" for
      // each thunk to mark the beginning of ARM code.
      if constexpr (std::is_same_v<E, ARM32>)
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
  if (this->strtab_size == 0)
    return;

  if constexpr (needs_thunk<E>) {
    ElfSym<E> *esym =
      (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset) + this->local_symtab_idx;
    memset(esym, 0, this->num_local_symtab * sizeof(ElfSym<E>));

    u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
    u8 *strtab = strtab_base + this->strtab_offset;

    if constexpr (std::is_same_v<E, ARM32>) {
      // ARM uses these symbols to mark the begining of Thumb code, ARM
      // code and data, respectively. Our thunk contains all of them.
      strtab += write_string(strtab, "$t");
      strtab += write_string(strtab, "$a");
      strtab += write_string(strtab, "$d");
    }

    for (std::unique_ptr<RangeExtensionThunk<E>> &thunk : thunks) {
      for (i64 i = 0; i < thunk->symbols.size(); i++) {
        Symbol<E> &sym = *thunk->symbols[i];

        auto write_esym = [&](i64 st_name, i64 off) {
          esym->st_name = st_name;
          esym->st_type = STT_FUNC;
          esym->st_shndx = this->shndx;
          esym->st_value = thunk->get_addr(i) + off;
          esym++;
        };

        write_esym(strtab - strtab_base, 0);

        strtab += write_string(strtab, sym.name()) - 1;
        strtab += write_string(strtab, "$thunk");

        // Emit "$t", "$a" and "$d" if ARM32.
        if constexpr (std::is_same_v<E, ARM32>) {
          write_esym(this->strtab_offset, 0);
          write_esym(this->strtab_offset + 3, 4);
          write_esym(this->strtab_offset + 6, 16);
        }
      }
    }
  }
}

template <typename E>
void GotSection<E>::add_got_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_got_idx(ctx, this->shdr.sh_size / sizeof(Word<E>));
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
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsdesc_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(supports_tlsdesc<E>);
  sym->set_tlsdesc_idx(ctx, this->shdr.sh_size / sizeof(Word<E>));
  this->shdr.sh_size += sizeof(Word<E>) * 2;
  tlsdesc_syms.push_back(sym);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsld(Context<E> &ctx) {
  if (tlsld_idx != -1)
    return;
  tlsld_idx = this->shdr.sh_size / sizeof(Word<E>);
  this->shdr.sh_size += sizeof(Word<E>) * 2;
}

template <typename E>
u64 GotSection<E>::get_tlsld_addr(Context<E> &ctx) const {
  assert(tlsld_idx != -1);
  return this->shdr.sh_addr + tlsld_idx * sizeof(Word<E>);
}

template <typename E>
i64 GotSection<E>::get_reldyn_size(Context<E> &ctx) const {
  i64 n = 0;
  for (GotEntry<E> &ent : get_entries(ctx))
    if (!ent.is_relr(ctx) && ent.r_type != R_NONE)
      n++;
  return n * sizeof(ElfRel<E>);
}

// Fill .got and .rel.dyn.
//
// .got is a linker-synthesized constant pool whose entry is of pointer
// size. If we know a correct value for an entry, we'll just set that value
// to the entry. Otherwise, we'll create a dynamic relocation and let the
// dynamic linker to fill the entry at laod-time.
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
std::vector<GotEntry<E>> GotSection<E>::get_entries(Context<E> &ctx) const {
  std::vector<GotEntry<E>> entries;

  for (Symbol<E> *sym : got_syms) {
    i64 idx = sym->get_got_idx(ctx);

    // If the symbol may not be defined within our output, let the
    // dynamic linker to resolve it.
    if (sym->is_imported) {
      entries.push_back({idx, 0, E::R_GLOB_DAT, sym});
      continue;
    }

    // IFUNC always needs to be fixed up by the dynamic linker.
    if (sym->is_ifunc()) {
      entries.push_back({idx, sym->get_addr(ctx, false), E::R_IRELATIVE});
      continue;
    }

    // If we know the address at address at link-time, fill that GOT entry
    // now. It may need a base relocation, though.
    if (ctx.arg.pic && sym->is_relative())
      entries.push_back({idx, sym->get_addr(ctx, false), E::R_RELATIVE});
    else
      entries.push_back({idx, sym->get_addr(ctx, false)});
  }

  for (Symbol<E> *sym : tlsgd_syms) {
    i64 idx = sym->get_tlsgd_idx(ctx);

    if (ctx.arg.is_static) {
      entries.push_back({idx, 1}); // One indicates the main executable file
      entries.push_back({idx + 1,
                         sym->get_addr(ctx) - ctx.tls_begin - E::tls_dtv_offset});
    } else {
      entries.push_back({idx, 0, E::R_DTPMOD, sym});
      entries.push_back({idx + 1, 0, E::R_DTPOFF, sym});
    }
  }

  if constexpr (supports_tlsdesc<E>) {
    for (Symbol<E> *sym : tlsdesc_syms) {
      // _TLS_MODULE_BASE_ is a linker-synthesized virtual symbol that
      // refers the begining of the TLS block.
      if (sym == ctx._TLS_MODULE_BASE_)
        entries.push_back({sym->get_tlsdesc_idx(ctx), 0, E::R_TLSDESC});
      else
        entries.push_back({sym->get_tlsdesc_idx(ctx), 0, E::R_TLSDESC, sym});
    }
  }

  for (Symbol<E> *sym : gottp_syms) {
    i64 idx = sym->get_gottp_idx(ctx);

    // If we know nothing about the symbol, let the dynamic linker
    // to fill the GOT entry.
    if (sym->is_imported) {
      entries.push_back({idx, 0, E::R_TPOFF, sym});
      continue;
    }

    // If we know the offset within the current thread vector,
    // let the dynamic linker to adjust it.
    if (ctx.arg.shared) {
      entries.push_back({idx, sym->get_addr(ctx) - ctx.tls_begin, E::R_TPOFF});
      continue;
    }

    // Otherwise, we know the offset from the thread pointer (TP) at
    // link-time, so we can fill the GOT entry directly.
    entries.push_back({idx, sym->get_addr(ctx) - ctx.tp_addr});
  }

  if (tlsld_idx != -1) {
    if (ctx.arg.is_static)
      entries.push_back({tlsld_idx, 1}); // One indicates the main executable
    else
      entries.push_back({tlsld_idx, 0, E::R_DTPMOD});
  }

  return entries;
}

// Fill .got and .rel.dyn.
template <typename E>
void GotSection<E>::copy_buf(Context<E> &ctx) {
  Word<E> *buf = (Word<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  // glibc on s390 and arm64 wrongly assumes GOT[0] refers _DYNAMIC.
  // We set the value only on s390 and arm64, so that no new code would
  // accidentally depend on the value of GOT[0].
  //
  // https://sourceware.org/bugzilla/show_bug.cgi?id=29662
  // https://sourceware.org/git/?p=glibc.git;a=commitdiff;h=43d06ed218fc8be58987bdfd00e21e5720f0b862
  if (ctx.dynamic)
    if (is_s390x<E> ||
        (std::is_same_v<E, ARM64> && ctx.arg.is_static && ctx.arg.pie))
      buf[0] = ctx.dynamic->shdr.sh_addr;

  ElfRel<E> *rel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset);

  for (GotEntry<E> &ent : get_entries(ctx)) {
    if (ent.is_relr(ctx) || ent.r_type == R_NONE) {
      buf[ent.idx] = ent.val;
    } else {
      *rel++ = ElfRel<E>(this->shdr.sh_addr + ent.idx * sizeof(Word<E>),
                         ent.r_type,
                         ent.sym ? ent.sym->get_dynsym_idx(ctx) : 0,
                         ent.val);

      if (ctx.arg.apply_dynamic_relocs)
        buf[ent.idx] = ent.val;
    }
  }
}

template <typename E>
void GotSection<E>::construct_relr(Context<E> &ctx) {
  assert(ctx.arg.pack_dyn_relocs_relr);

  std::vector<u64> pos;
  for (GotEntry<E> &ent : get_entries(ctx))
    if (ent.is_relr(ctx))
      pos.push_back(ent.idx * sizeof(Word<E>));

  relr = encode_relr(pos, sizeof(Word<E>));
}

template <typename E>
void GotSection<E>::compute_symtab_size(Context<E> &ctx) {
  if (ctx.arg.strip_all || ctx.arg.retain_symbols_file)
    return;

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
  if (this->strtab_size == 0)
    return;

  ElfSym<E> *esym =
    (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset) + this->local_symtab_idx;
  memset(esym, 0, this->num_local_symtab * sizeof(ElfSym<E>));

  u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
  u8 *strtab = strtab_base + this->strtab_offset;

  auto write = [&](std::string_view name, std::string_view suffix, i64 value) {
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
  if constexpr (!is_ppc<E>) {
    Word<E> *buf = (Word<E> *)(ctx.buf + this->shdr.sh_offset);
    memset(buf, 0, this->shdr.sh_size);

    // The first slot of .got.plt points to _DYNAMIC, as requested by
    // the psABI. The second and the third slots are reserved by the psABI.
    static_assert(HDR_SIZE / sizeof(Word<E>) == 3);

    if (ctx.dynamic)
      buf[0] = ctx.dynamic->shdr.sh_addr;

    for (i64 i = 3; [[maybe_unused]] Symbol<E> *sym : ctx.plt->symbols)
      buf[i++] = ctx.plt->shdr.sh_addr;
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
    this->shdr.sh_size = E::plt_hdr_size + symbols.size() * E::plt_size;
}

template <typename E>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + ctx.plt->shdr.sh_offset;
  write_plt_header(ctx, buf);

  for (i64 i = 0; i < symbols.size(); i++)
    write_plt_entry(ctx, buf + E::plt_hdr_size + i * E::plt_size, *symbols[i]);
}

template <typename E>
void PltSection<E>::compute_symtab_size(Context<E> &ctx) {
  if (ctx.arg.strip_all || ctx.arg.retain_symbols_file)
    return;

  this->num_local_symtab = symbols.size();
  this->strtab_size = 0;

  for (Symbol<E> *sym : symbols)
    this->strtab_size += sym->name().size() + sizeof("$plt");
}

template <typename E>
void PltSection<E>::populate_symtab(Context<E> &ctx) {
  if (this->strtab_size == 0)
    return;

  ElfSym<E> *esym =
    (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset) + this->local_symtab_idx;
  memset(esym, 0, symbols.size() * sizeof(ElfSym<E>));

  u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
  u8 *strtab = strtab_base + this->strtab_offset;

  for (Symbol<E> *sym : symbols) {
    esym->st_name = strtab - strtab_base;
    esym->st_type = STT_FUNC;
    esym->st_shndx = this->shndx;
    esym->st_value = sym->get_plt_addr(ctx);
    esym++;

    strtab += write_string(strtab, sym->name()) - 1;
    strtab += write_string(strtab, "$plt");
  }
}

template <typename E>
void PltGotSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(!sym->has_plt(ctx));
  assert(sym->has_got(ctx));

  sym->set_pltgot_idx(ctx, this->shdr.sh_size / E::pltgot_size);
  this->shdr.sh_size += E::pltgot_size;
  symbols.push_back(sym);
}

template <typename E>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + ctx.pltgot->shdr.sh_offset;
  for (i64 i = 0; i < symbols.size(); i++)
    write_pltgot_entry(ctx, buf + i * E::pltgot_size, *symbols[i]);
}

template <typename E>
void PltGotSection<E>::compute_symtab_size(Context<E> &ctx) {
  if (ctx.arg.strip_all || ctx.arg.retain_symbols_file)
    return;

  this->num_local_symtab = symbols.size();
  this->strtab_size = 0;

  for (Symbol<E> *sym : symbols)
    this->strtab_size += sym->name().size() + sizeof("$pltgot");
}

template <typename E>
void PltGotSection<E>::populate_symtab(Context<E> &ctx) {
  if (this->strtab_size == 0)
    return;

  ElfSym<E> *esym =
    (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset) + this->local_symtab_idx;
  memset(esym, 0, symbols.size() * sizeof(ElfSym<E>));

  u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
  u8 *strtab = strtab_base + this->strtab_offset;

  for (Symbol<E> *sym : symbols) {
    esym->st_name = strtab - strtab_base;
    esym->st_type = STT_FUNC;
    esym->st_shndx = this->shndx;
    esym->st_value = sym->get_plt_addr(ctx);
    esym++;

    strtab += write_string(strtab, sym->name()) - 1;
    strtab += write_string(strtab, "$pltgot");
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

  for (i64 i = 0; i < ctx.plt->symbols.size(); i++) {
    Symbol<E> &sym = *ctx.plt->symbols[i];

    // SPARC doesn't have a .got.plt because its role is merged to .plt.
    // On SPARC, .plt is writable (!) and the dynamic linker directly
    // modify its machine instructions as it resolves dynamic symbols.
    // Therefore, it doesn't need a separate section to store the symbol
    // resolution results. That is of course horrible from the security
    // point of view, though.
    u64 addr = is_sparc<E> ? sym.get_plt_addr(ctx) : sym.get_gotplt_addr(ctx);

    buf[i] = ElfRel<E>(addr, E::R_JUMP_SLOT, sym.get_dynsym_idx(ctx), 0);
  }
}

template<typename E>
ElfSym<E> to_output_esym(Context<E> &ctx, Symbol<E> &sym) {
  ElfSym<E> esym;
  memset(&esym, 0, sizeof(esym));
  esym.st_type = sym.esym().st_type;

  if (sym.is_local())
    esym.st_bind = STB_LOCAL;
  else if (sym.is_weak)
    esym.st_bind = STB_WEAK;
  else if (sym.file->is_dso)
    esym.st_bind = STB_GLOBAL;
  else
    esym.st_bind = sym.esym().st_bind;

  auto get_st_shndx = [](Symbol<E> &sym) -> u32 {
    if (SectionFragment<E> *frag = sym.get_frag())
      if (frag->is_alive)
        return frag->output_section.shndx;

    if (InputSection<E> *isec = sym.get_input_section())
      if (isec->is_alive)
        return isec->output_section->shndx;
    return SHN_UNDEF;
  };

  if (sym.has_copyrel) {
    esym.st_shndx =
      sym.copyrel_readonly ? ctx.copyrel_relro->shndx : ctx.copyrel->shndx;
    esym.st_value = sym.get_addr(ctx);
    esym.st_size = sym.esym().st_size;
  } else if (sym.file->is_dso || sym.esym().is_undef()) {
    esym.st_shndx = SHN_UNDEF;
    esym.st_value = sym.is_canonical ? sym.get_plt_addr(ctx) : 0;
    esym.st_size = 0;
  } else if (Chunk<E> *osec = sym.get_output_section()) {
    // Linker-synthesized symbols
    esym.st_shndx = osec->shndx;
    esym.st_value = sym.get_addr(ctx);
    esym.st_size = sym.esym().st_size;
  } else if (SectionFragment<E> *frag = sym.get_frag()) {
    // Section fragment
    esym.st_shndx = frag->output_section.shndx;
    esym.st_value = sym.get_addr(ctx);
    esym.st_size = 0;
  } else if (!sym.get_input_section()) {
    // Absolute symbol
    esym.st_shndx = SHN_ABS;
    esym.st_value = sym.get_addr(ctx);
    esym.st_size = sym.esym().st_size;
  } else if (sym.get_type() == STT_TLS) {
    esym.st_shndx = get_st_shndx(sym);
    esym.st_value = sym.get_addr(ctx) - ctx.tls_begin;
    esym.st_size = sym.esym().st_size;
  } else {
    esym.st_visibility = sym.visibility;
    esym.st_shndx = get_st_shndx(sym);
    esym.st_value = sym.get_addr(ctx, false);
    esym.st_size = sym.esym().st_size;
  }
  return esym;
}

template <typename E>
void DynsymSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  if (symbols.empty())
    symbols.resize(1);

  if (sym->get_dynsym_idx(ctx) != -1)
    return;
  sym->set_dynsym_idx(ctx, -2);
  symbols.push_back(sym);
}

template <typename E>
void DynsymSection<E>::finalize(Context<E> &ctx) {
  Timer t(ctx, "DynsymSection::finalize");
  if (symbols.empty())
    return;

  // We need a stable sort for build reproducibility, but parallel_sort
  // isn't stable, so we use this struct to make it stable.
  struct T {
    Symbol<E> *sym = nullptr;
    u32 hash = 0;
    i32 idx = 0;
  };

  // Sort symbols. In any symtab, local symbols must precede global symbols.
  // We also place undefined symbols before defined symbols for .gnu.hash.
  // Defined symbols are sorted by their hashes for .gnu.hash.
  std::vector<T> vec(symbols.size());
  i64 num_buckets = 0;

  if (ctx.gnu_hash) {
    // Count the number of exported symbols to compute the size of .gnu.hash.
    i64 num_exported = 0;
    for (i64 i = 1; i < symbols.size(); i++)
      if (symbols[i]->is_exported)
        num_exported++;

    num_buckets = num_exported / ctx.gnu_hash->LOAD_FACTOR + 1;
    ctx.gnu_hash->num_buckets = num_buckets;
  }

  tbb::parallel_for((i64)1, (i64)symbols.size(), [&](i64 i) {
    Symbol<E> *sym = symbols[i];
    vec[i].sym = sym;
    if (ctx.gnu_hash && sym->is_exported)
      vec[i].hash = djb_hash(sym->name()) % num_buckets;
    vec[i].idx = i;
  });

  tbb::parallel_sort(vec.begin() + 1, vec.end(), [&](const T &a, const T &b) {
    return std::tuple(!a.sym->is_local(), (bool)a.sym->is_exported, a.hash, a.idx) <
           std::tuple(!b.sym->is_local(), (bool)b.sym->is_exported, b.hash, b.idx);
  });

  ctx.dynstr->dynsym_offset = ctx.dynstr->shdr.sh_size;

  for (i64 i = 1; i < symbols.size(); i++) {
    symbols[i] = vec[i].sym;
    symbols[i]->set_dynsym_idx(ctx, i);
    ctx.dynstr->shdr.sh_size += symbols[i]->name().size() + 1;
  }

  // ELF's symbol table sh_info holds the offset of the first global symbol.
  auto first_global =
    std::partition_point(symbols.begin() + 1, symbols.end(), [](Symbol<E> *sym) {
      return sym->is_local();
    });
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

    esym = to_output_esym(ctx, sym);
    esym.st_name = name_offset;
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

  i64 num_slots = ctx.dynsym->symbols.size();
  U32<E> *hdr = (U32<E> *)base;
  U32<E> *buckets = (U32<E> *)(base + 8);
  U32<E> *chains = buckets + num_slots;

  hdr[0] = hdr[1] = num_slots;

  for (i64 i = 1; i < ctx.dynsym->symbols.size(); i++) {
    Symbol<E> *sym = ctx.dynsym->symbols[i];
    i64 idx = elf_hash(sym->name()) % num_slots;
    chains[sym->get_dynsym_idx(ctx)] = buckets[idx];
    buckets[idx] = sym->get_dynsym_idx(ctx);
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
  i64 exported_offset = ctx.dynsym->symbols.size() - syms.size();

  *(U32<E> *)base = num_buckets;
  *(U32<E> *)(base + 4) = exported_offset;
  *(U32<E> *)(base + 8) = num_bloom;
  *(U32<E> *)(base + 12) = BLOOM_SHIFT;

  std::vector<u32> hashes(syms.size());
  for (i64 i = 0; i < syms.size(); i++)
    hashes[i] = djb_hash(syms[i]->name());

  // Write a bloom filter
  Word<E> *bloom = (Word<E> *)(base + HEADER_SIZE);
  for (i64 hash : hashes) {
    constexpr i64 word_bits = sizeof(Word<E>) * 8;
    i64 idx = (hash / word_bits) % num_bloom;
    bloom[idx] |= 1LL << (hash % word_bits);
    bloom[idx] |= 1LL << ((hash >> BLOOM_SHIFT) % word_bits);
  }

  // Write hash bucket indices
  U32<E> *buckets = (U32<E> *)(bloom + num_bloom);
  for (i64 i = 0; i < hashes.size(); i++) {
    i64 idx = hashes[i] % num_buckets;
    if (!buckets[idx])
      buckets[idx] = i + exported_offset;
  }

  // Write a hash table
  U32<E> *table = buckets + num_buckets;
  for (i64 i = 0; i < syms.size(); i++) {
    bool is_last = false;
    if (i == syms.size() - 1 ||
        (hashes[i] % num_buckets) != (hashes[i + 1] % num_buckets))
      is_last = true;

    if (is_last)
      table[i] = hashes[i] | 1;
    else
      table[i] = hashes[i] & ~1;
  }
}

template <typename E>
MergedSection<E>::MergedSection(std::string_view name, u64 flags, u32 type) {
  this->name = name;
  this->shdr.sh_flags = flags;
  this->shdr.sh_type = type;
}

template <typename E>
MergedSection<E> *
MergedSection<E>::get_instance(Context<E> &ctx, std::string_view name,
                               u64 type, u64 flags) {
  name = get_output_name(ctx, name, flags);
  flags = flags & ~(u64)SHF_GROUP & ~(u64)SHF_MERGE & ~(u64)SHF_STRINGS &
          ~(u64)SHF_COMPRESSED;

  auto find = [&]() -> MergedSection * {
    for (std::unique_ptr<MergedSection<E>> &osec : ctx.merged_sections)
      if (std::tuple(name, flags, type) ==
          std::tuple(osec->name, osec->shdr.sh_flags, osec->shdr.sh_type))
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

  MergedSection *osec = new MergedSection(name, flags, type);
  ctx.merged_sections.emplace_back(osec);
  return osec;
}

template <typename E>
SectionFragment<E> *
MergedSection<E>::insert(std::string_view data, u64 hash, i64 p2align) {
  std::call_once(once_flag, [&] {
    // We aim 2/3 occupation ratio
    map.resize(estimator.get_cardinality() * 3 / 2);
  });

  SectionFragment<E> *frag;
  bool inserted;
  std::tie(frag, inserted) = map.insert(data, hash, SectionFragment(this));
  assert(frag);

  update_maximum(frag->p2align, p2align);
  return frag;
}

template <typename E>
void MergedSection<E>::assign_offsets(Context<E> &ctx) {
  std::vector<i64> sizes(map.NUM_SHARDS);
  std::vector<i64> max_p2aligns(map.NUM_SHARDS);
  shard_offsets.resize(map.NUM_SHARDS + 1);

  i64 shard_size = map.nbuckets / map.NUM_SHARDS;

  tbb::parallel_for((i64)0, map.NUM_SHARDS, [&](i64 i) {
    struct KeyVal {
      std::string_view key;
      SectionFragment<E> *val;
    };

    std::vector<KeyVal> fragments;
    fragments.reserve(shard_size);

    for (i64 j = shard_size * i; j < shard_size * (i + 1); j++)
      if (SectionFragment<E> &frag = map.values[j]; frag.is_alive)
        fragments.push_back({{map.keys[j], map.key_sizes[j]}, &frag});

    // Sort fragments to make output deterministic.
    tbb::parallel_sort(fragments.begin(), fragments.end(),
                       [](const KeyVal &a, const KeyVal &b) {
      return std::tuple{a.val->p2align.load(), a.key.size(), a.key} <
             std::tuple{b.val->p2align.load(), b.key.size(), b.key};
    });

    // Assign offsets.
    i64 offset = 0;
    i64 p2align = 0;

    for (KeyVal &kv : fragments) {
      SectionFragment<E> &frag = *kv.val;
      offset = align_to(offset, 1 << frag.p2align);
      frag.offset = offset;
      offset += kv.key.size();
      p2align = std::max<i64>(p2align, frag.p2align);
    }

    sizes[i] = offset;
    max_p2aligns[i] = p2align;

    static Counter merged_strings("merged_strings");
    merged_strings += fragments.size();
  });

  i64 p2align = 0;
  for (i64 x : max_p2aligns)
    p2align = std::max(p2align, x);

  for (i64 i = 1; i < map.NUM_SHARDS + 1; i++)
    shard_offsets[i] =
      align_to(shard_offsets[i - 1] + sizes[i - 1], 1 << p2align);

  tbb::parallel_for((i64)1, map.NUM_SHARDS, [&](i64 i) {
    for (i64 j = shard_size * i; j < shard_size * (i + 1); j++)
      if (SectionFragment<E> &frag = map.values[j]; frag.is_alive)
        frag.offset += shard_offsets[i];
  });

  this->shdr.sh_size = shard_offsets[map.NUM_SHARDS];
  this->shdr.sh_addralign = 1 << p2align;
}

template <typename E>
void MergedSection<E>::copy_buf(Context<E> &ctx) {
  write_to(ctx, ctx.buf + this->shdr.sh_offset);
}

template <typename E>
void MergedSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  i64 shard_size = map.nbuckets / map.NUM_SHARDS;

  tbb::parallel_for((i64)0, map.NUM_SHARDS, [&](i64 i) {
    memset(buf + shard_offsets[i], 0, shard_offsets[i + 1] - shard_offsets[i]);

    for (i64 j = shard_size * i; j < shard_size * (i + 1); j++)
      if (SectionFragment<E> &frag = map.values[j]; frag.is_alive)
        memcpy(buf + frag.offset, map.keys[j], map.key_sizes[j]);
  });
}

template <typename E>
void MergedSection<E>::print_stats(Context<E> &ctx) {
  i64 used = 0;
  for (i64 i = 0; i < map.nbuckets; i++)
    if (map.keys[i])
      used++;

  SyncOut(ctx) << this->name
               << " estimation=" << estimator.get_cardinality()
               << " actual=" << used;
}

template <typename E>
void EhFrameSection<E>::construct(Context<E> &ctx) {
  Timer t(ctx, "eh_frame");

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
      if (cie.equals(*leader))
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

  HdrEntry *eh_hdr_begin = nullptr;
  if (ctx.arg.eh_frame_hdr)
    eh_hdr_begin = (HdrEntry *)(ctx.buf + ctx.eh_frame_hdr->shdr.sh_offset +
                   EhFrameHdrSection<E>::HEADER_SIZE);

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    // Copy CIEs.
    for (CieRecord<E> &cie : file->cies) {
      if (!cie.is_leader)
        continue;

      std::string_view contents = cie.get_contents();
      memcpy(base + cie.output_offset, contents.data(), contents.size());

      for (const ElfRel<E> &rel : cie.get_rels()) {
        if (rel.r_type == R_NONE)
          continue;
        assert(rel.r_offset - cie.input_offset < contents.size());
        u64 loc = cie.output_offset + rel.r_offset - cie.input_offset;
        u64 val = file->symbols[rel.r_sym]->get_addr(ctx);
        u64 addend = cie.input_section.get_addend(rel);
        apply_reloc(ctx, rel, loc, val + addend);
      }
    }

    // Copy FDEs.
    for (i64 i = 0; i < file->fdes.size(); i++) {
      FdeRecord<E> &fde = file->fdes[i];
      i64 offset = file->fde_offset + fde.output_offset;

      std::string_view contents = fde.get_contents(*file);
      memcpy(base + offset, contents.data(), contents.size());

      CieRecord<E> &cie = file->cies[fde.cie_idx];
      *(U32<E> *)(base + offset + 4) = offset + 4 - cie.output_offset;
      bool is_first = true;

      for (const ElfRel<E> &rel : fde.get_rels(*file)) {
        if (rel.r_type == R_NONE)
          continue;

        assert(rel.r_offset - fde.input_offset < contents.size());
        u64 loc = offset + rel.r_offset - fde.input_offset;
        u64 val = file->symbols[rel.r_sym]->get_addr(ctx);
        u64 addend = cie.input_section.get_addend(rel);
        apply_reloc(ctx, rel, loc, val + addend);

        if (eh_hdr_begin && is_first) {
          // Write to .eh_frame_hdr
          HdrEntry &ent = eh_hdr_begin[file->fde_idx + i];
          u64 sh_addr = ctx.eh_frame_hdr->shdr.sh_addr;
          ent.init_addr = val + addend - sh_addr;
          ent.fde_addr = this->shdr.sh_addr + offset - sh_addr;
          is_first = false;
        }
      }
    }
  });

  // Write a terminator.
  *(U32<E> *)(base + this->shdr.sh_size - 4) = 0;

  // Sort .eh_frame_hdr contents.
  if (eh_hdr_begin) {
    tbb::parallel_sort(eh_hdr_begin, eh_hdr_begin + ctx.eh_frame_hdr->num_fdes,
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
void CopyrelSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  if (sym->has_copyrel)
    return;

  assert(!ctx.arg.shared);
  assert(sym->file->is_dso);

  this->shdr.sh_size = align_to(this->shdr.sh_size, this->shdr.sh_addralign);
  sym->value = this->shdr.sh_size;
  sym->has_copyrel = true;

  this->shdr.sh_size += sym->esym().st_size;
  symbols.push_back(sym);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void CopyrelSection<E>::update_shdr(Context<E> &ctx) {
  // SHT_NOBITS sections (i.e. BSS sections) have to be at the end of
  // a segment, so a .copyrel.rel.ro usually requires one extra
  // segment for it. We turn a .coyprel.rel.ro into a regular section
  // if it is very small to avoid the cost of the extra segment.
  constexpr i64 threshold = 4096;
  if (is_relro && ctx.arg.z_relro && this->shdr.sh_size < threshold)
    this->shdr.sh_type = SHT_PROGBITS;
}

template <typename E>
void CopyrelSection<E>::copy_buf(Context<E> &ctx) {
  if (this->shdr.sh_type == SHT_PROGBITS)
    memset(ctx.buf + this->shdr.sh_offset, 0, this->shdr.sh_size);
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

template <typename E>
void VerneedSection<E>::construct(Context<E> &ctx) {
  Timer t(ctx, "fill_verneed");

  if (ctx.dynsym->symbols.empty())
    return;

  // Create a list of versioned symbols and sort by file and version.
  std::vector<Symbol<E> *> syms(ctx.dynsym->symbols.begin() + 1,
                                ctx.dynsym->symbols.end());

  std::erase_if(syms, [](Symbol<E> *sym) {
    return !sym->file->is_dso || sym->ver_idx <= VER_NDX_LAST_RESERVED;
  });

  if (syms.empty())
    return;

  sort(syms, [](Symbol<E> *a, Symbol<E> *b) {
    return std::tuple(((SharedFile<E> *)a->file)->soname, a->ver_idx) <
           std::tuple(((SharedFile<E> *)b->file)->soname, b->ver_idx);
  });

  // Resize of .gnu.version
  ctx.versym->contents.resize(ctx.dynsym->symbols.size(), 1);
  ctx.versym->contents[0] = 0;

  // Allocate a large enough buffer for .gnu.version_r.
  contents.resize((sizeof(ElfVerneed<E>) + sizeof(ElfVernaux<E>)) * syms.size());

  // Fill .gnu.version_r.
  u8 *buf = (u8 *)&contents[0];
  u8 *ptr = buf;
  ElfVerneed<E> *verneed = nullptr;
  ElfVernaux<E> *aux = nullptr;

  u16 veridx = VER_NDX_LAST_RESERVED + ctx.arg.version_definitions.size();

  auto start_group = [&](InputFile<E> *file) {
    this->shdr.sh_info++;
    if (verneed)
      verneed->vn_next = ptr - (u8 *)verneed;

    verneed = (ElfVerneed<E> *)ptr;
    ptr += sizeof(*verneed);
    verneed->vn_version = 1;
    verneed->vn_file = ctx.dynstr->find_string(((SharedFile<E> *)file)->soname);
    verneed->vn_aux = sizeof(ElfVerneed<E>);
    aux = nullptr;
  };

  auto add_entry = [&](Symbol<E> *sym) {
    verneed->vn_cnt++;

    if (aux)
      aux->vna_next = sizeof(ElfVernaux<E>);
    aux = (ElfVernaux<E> *)ptr;
    ptr += sizeof(*aux);

    std::string_view verstr = sym->get_version();
    aux->vna_hash = elf_hash(verstr);
    aux->vna_other = ++veridx;
    aux->vna_name = ctx.dynstr->add_string(verstr);
  };

  for (i64 i = 0; i < syms.size(); i++) {
    if (i == 0 || syms[i - 1]->file != syms[i]->file) {
      start_group(syms[i]->file);
      add_entry(syms[i]);
    } else if (syms[i - 1]->ver_idx != syms[i]->ver_idx) {
      add_entry(syms[i]);
    }

    ctx.versym->contents[syms[i]->get_dynsym_idx(ctx)] = veridx;
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
  ctx.versym->contents.resize(ctx.dynsym->symbols.size(), 1);
  ctx.versym->contents[0] = 0;

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

  std::string_view basename = ctx.arg.soname.empty() ?
    ctx.arg.output : ctx.arg.soname;
  write(basename, 1, VER_FLG_BASE);

  i64 idx = 2;
  for (std::string_view verstr : ctx.arg.version_definitions)
    write(verstr, idx++, 0);

  for (Symbol<E> *sym : std::span<Symbol<E> *>(ctx.dynsym->symbols).subspan(1))
    ctx.versym->contents[sym->get_dynsym_idx(ctx)] = sym->ver_idx;
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

template <typename E>
static void compute_sha256(Context<E> &ctx, i64 offset) {
  u8 *buf = ctx.buf;
  i64 filesize = ctx.output_file->filesize;

  i64 shard_size = 4096 * 1024;
  i64 num_shards = align_to(filesize, shard_size) / shard_size;
  std::vector<u8> shards(num_shards * SHA256_SIZE);

  tbb::parallel_for((i64)0, num_shards, [&](i64 i) {
    u8 *begin = buf + shard_size * i;
    u8 *end = (i == num_shards - 1) ? buf + filesize : begin + shard_size;
    sha256_hash(begin, end - begin, shards.data() + i * SHA256_SIZE);

#ifndef _WIN32
    // We call munmap early for each chunk so that the last munmap
    // gets cheaper. We assume that the .note.build-id section is
    // at the beginning of an output file. This is an ugly performance
    // hack, but we can save about 30 ms for a 2 GiB output.
    if (i > 0 && ctx.output_file->is_mmapped)
      munmap(begin, end - begin);
#endif
   });

  assert(ctx.arg.build_id.size() <= SHA256_SIZE);

  u8 digest[SHA256_SIZE];
  sha256_hash(shards.data(), shards.size(), digest);
  memcpy(buf + offset, digest, ctx.arg.build_id.size());

#ifndef _WIN32
  if (ctx.output_file->is_mmapped) {
    munmap(buf, std::min(filesize, shard_size));
    ctx.output_file->is_unmapped = true;
  }
#endif
}

template <typename E>
void BuildIdSection<E>::write_buildid(Context<E> &ctx) {
  Timer t(ctx, "build_id");

  switch (ctx.arg.build_id.kind) {
  case BuildId::HEX:
    write_vector(ctx.buf + this->shdr.sh_offset + HEADER_SIZE,
                 ctx.arg.build_id.value);
    return;
  case BuildId::HASH:
    // Modern x86 processors have purpose-built instructions to accelerate
    // SHA256 computation, and SHA256 outperforms MD5 on such computers.
    // So, we always compute SHA256 and truncate it if smaller digest was
    // requested.
    compute_sha256(ctx, this->shdr.sh_offset + HEADER_SIZE);
    return;
  case BuildId::UUID: {
    std::array<u8, 16> uuid = get_uuid_v4();
    memcpy(ctx.buf + this->shdr.sh_offset + HEADER_SIZE, uuid.data(), 16);
    return;
  }
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

template <typename E>
void NotePropertySection<E>::update_shdr(Context<E> &ctx) {
  features = -1;
  for (ObjectFile<E> *file : ctx.objs)
    features &= file->features;

  if (ctx.arg.z_ibt)
    features |= GNU_PROPERTY_X86_FEATURE_1_IBT;
  if (ctx.arg.z_shstk)
    features |= GNU_PROPERTY_X86_FEATURE_1_SHSTK;

  if (features != 0 && features != -1)
    this->shdr.sh_size = E::is_64 ? 32 : 28;
}

template <typename E>
void NotePropertySection<E>::copy_buf(Context<E> &ctx) {
  U32<E> *buf = (U32<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  buf[0] = 4;                              // Name size
  buf[1] = E::is_64 ? 16 : 12;             // Content size
  buf[2] = NT_GNU_PROPERTY_TYPE_0;         // Type
  memcpy(buf + 3, "GNU", 4);               // Name
  buf[4] = GNU_PROPERTY_X86_FEATURE_1_AND; // Feature type
  buf[5] = 4;                              // Feature size
  buf[6] = features;                       // Feature flags
}

// This page explains the format of .gdb_index:
// https://sourceware.org/gdb/onlinedocs/gdb/Index-Section-Format.html
template <typename E>
void GdbIndexSection<E>::construct(Context<E> &ctx) {
  Timer t(ctx, "GdbIndexSection::construct");

  std::atomic_bool has_debug_info = false;

  // Read debug sections
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    if (file->debug_info) {
      // Read compilation units from .debug_info.
      file->compunits = read_compunits(ctx, *file);

      // Count the number of address areas contained in this file.
      file->num_areas = estimate_address_areas(ctx, *file);
      has_debug_info = true;
    }
  });

  if (!has_debug_info)
    return;

  // Initialize `area_offset` and `compunits_idx`.
  for (i64 i = 0; i < ctx.objs.size() - 1; i++) {
    ctx.objs[i + 1]->area_offset =
      ctx.objs[i]->area_offset + ctx.objs[i]->num_areas * 20;
    ctx.objs[i + 1]->compunits_idx =
      ctx.objs[i]->compunits_idx + ctx.objs[i]->compunits.size();
  }

  // Read .debug_gnu_pubnames and .debug_gnu_pubtypes.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->gdb_names = read_pubnames(ctx, *file);
  });

  // Estimate the unique number of pubnames.
  HyperLogLog estimator;
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    HyperLogLog e;
    for (GdbIndexName &name : file->gdb_names)
      e.insert(name.hash);
    estimator.merge(e);
  });

  // Uniquify pubnames by inserting all name strings into a concurrent
  // hashmap.
  map.resize(estimator.get_cardinality() * 2);
  tbb::enumerable_thread_specific<i64> num_names;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (GdbIndexName &name : file->gdb_names) {
      MapEntry *ent;
      bool inserted;
      std::tie(ent, inserted) = map.insert(name.name, name.hash, {file, name.hash});
      if (inserted)
        num_names.local()++;

      ObjectFile<E> *old_val = ent->owner;
      while (file->priority < old_val->priority &&
             !ent->owner.compare_exchange_weak(old_val, file));

      ent->num_attrs++;
      name.entry_idx = ent - map.values;
    }
  });

  // Assign offsets for names and attributes within each file.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (GdbIndexName &name : file->gdb_names) {
      MapEntry &ent = map.values[name.entry_idx];
      if (ent.owner == file) {
        ent.attr_offset = file->attrs_size;
        file->attrs_size += (ent.num_attrs + 1) * 4;
        ent.name_offset = file->names_size;
        file->names_size += name.name.size() + 1;
      }
    }
  });

  // Compute per-file name and attributes offsets.
  for (i64 i = 0; i < ctx.objs.size() - 1; i++)
    ctx.objs[i + 1]->attrs_offset =
      ctx.objs[i]->attrs_offset + ctx.objs[i]->attrs_size;

  ctx.objs[0]->names_offset =
    ctx.objs.back()->attrs_offset + ctx.objs.back()->attrs_size;

  for (i64 i = 0; i < ctx.objs.size() - 1; i++)
    ctx.objs[i + 1]->names_offset =
      ctx.objs[i]->names_offset + ctx.objs[i]->names_size;

  // .gdb_index contains an on-disk hash table for pubnames and
  // pubtypes. We aim 75% utilization. As per the format specification,
  // It must be a power of two.
  i64 num_symtab_entries =
    std::max<i64>(bit_ceil(num_names.combine(std::plus()) * 4 / 3), 16);

  // Now that we can compute the size of this section.
  ObjectFile<E> &last = *ctx.objs.back();
  i64 compunits_size = (last.compunits_idx + last.compunits.size()) * 16;
  i64 areas_size = last.area_offset + last.num_areas * 20;
  i64 offset = sizeof(header);

  header.cu_list_offset = offset;
  offset += compunits_size;

  header.cu_types_offset = offset;
  header.areas_offset = offset;
  offset += areas_size;

  header.symtab_offset = offset;
  offset += num_symtab_entries * 8;

  header.const_pool_offset = offset;
  offset += last.names_offset + last.names_size;

  this->shdr.sh_size = offset;
}

template <typename E>
void GdbIndexSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  // Write section header.
  memcpy(buf, &header, sizeof(header));
  buf += sizeof(header);

  // Write compilation unit list.
  for (ObjectFile<E> *file : ctx.objs) {
    if (file->debug_info) {
      u64 offset = file->debug_info->offset;
      for (std::string_view cu : file->compunits) {
        *(ul64 *)buf = offset;
        *(ul64 *)(buf + 8) = cu.size();
        buf += 16;
        offset += cu.size();
      }
    }
  }

  // Skip address areas. It'll be filled by write_address_areas.
  buf += header.symtab_offset - header.areas_offset;

  // Write an on-disk hash table for names.
  u32 symtab_size = header.const_pool_offset - header.symtab_offset;
  memset(buf, 0, symtab_size);

  assert(has_single_bit(symtab_size / 8));
  u32 mask = symtab_size / 8 - 1;

  for (i64 i = 0; i < map.nbuckets; i++) {
    if (map.has_key(i)) {
      u32 hash = map.values[i].hash;
      u32 step = (hash & mask) | 1;
      u32 j = hash & mask;

      while (*(U32<E> *)(buf + j * 8))
        j = (j + step) & mask;

      ObjectFile<E> &file = *map.values[i].owner;
      *(ul32 *)(buf + j * 8) = file.names_offset + map.values[i].name_offset;
      *(ul32 *)(buf + j * 8 + 4) = file.attrs_offset + map.values[i].attr_offset;
    }
  }

  buf += symtab_size;

  // Write CU vector
  memset(buf, 0, ctx.objs[0]->names_offset);

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    std::atomic_uint32_t *attrs = (std::atomic_uint32_t *)buf;

    for (GdbIndexName &name : file->gdb_names) {
      MapEntry &ent = map.values[name.entry_idx];
      u32 idx = (ent.owner.load()->attrs_offset + ent.attr_offset) / 4;
      attrs[idx + ++attrs[idx]] = name.attr;
    }
  });

  // Sort CU vector for build reproducibility
  const i64 shard_size = map.nbuckets / map.NUM_SHARDS;

  tbb::parallel_for((i64)0, (i64)map.NUM_SHARDS, [&](i64 i) {
    U32<E> *attrs = (U32<E> *)buf;

    for (i64 j = shard_size * i; j < shard_size * (i + 1); j++) {
      if (map.has_key(j)) {
        MapEntry &ent = map.values[j];
        u32 idx = (ent.owner.load()->attrs_offset + ent.attr_offset) / 4;
        U32<E> *start = attrs + idx + 1;
        std::sort(start, start + attrs[idx]);
      }
    }
  });

  // .gdb_index contents are little-endian, so swap bytes if big-endian.
  if constexpr (std::endian::native == std::endian::big)
    for (i64 i = 0; i < ctx.objs[0]->names_offset; i += 4)
      *(u32 *)(buf + i) = bswap(*(u32 *)(buf + i));

  // Write pubnames and pubtypes.
  tbb::parallel_for((i64)0, (i64)map.NUM_SHARDS, [&](i64 i) {
    for (i64 j = shard_size * i; j < shard_size * (i + 1); j++) {
      if (map.has_key(j)) {
        ObjectFile<E> &file = *map.values[j].owner;
        std::string_view name{map.keys[j], map.key_sizes[j]};
        write_string(buf + file.names_offset + map.values[j].name_offset, name);
      }
    }
  });
}

template <typename E>
void GdbIndexSection<E>::write_address_areas(Context<E> &ctx) {
  Timer t(ctx, "GdbIndexSection::write_address_areas");

  if (this->shdr.sh_size == 0)
    return;

  u8 *base = ctx.buf + this->shdr.sh_offset;

  for (Chunk<E> *chunk : ctx.chunks) {
    std::string_view name = chunk->name;
    if (name == ".debug_info")
      ctx.debug_info = chunk;
    if (name == ".debug_abbrev")
      ctx.debug_abbrev = chunk;
    if (name == ".debug_ranges")
      ctx.debug_ranges = chunk;
    if (name == ".debug_addr")
      ctx.debug_addr = chunk;
    if (name == ".debug_rnglists")
      ctx.debug_rnglists = chunk;
  }

  assert(ctx.debug_info);
  assert(ctx.debug_abbrev);

  struct Entry {
    ul64 start;
    ul64 end;
    ul32 attr;
  };

  // Read address ranges from debug sections and copy them to .gdb_index.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    if (!file->debug_info)
      return;

    Entry *begin = (Entry *)(base + header.areas_offset + file->area_offset);
    Entry *e = begin;
    u64 offset = file->debug_info->offset;

    for (i64 i = 0; i < file->compunits.size(); i++) {
      std::vector<u64> addrs = read_address_areas(ctx, *file, offset);

      for (i64 j = 0; j < addrs.size(); j += 2) {
        // Skip an empty range
        if (addrs[j] == addrs[j + 1])
          continue;

        // Gdb crashes if there are entries with address 0.
        if (addrs[j] == 0)
          continue;

        assert(e < begin + file->num_areas);
        e->start = addrs[j];
        e->end = addrs[j + 1];
        e->attr = file->compunits_idx + i;
        e++;
      }
      offset += file->compunits[i].size();
    }

    // Fill trailing null entries with dummy values because gdb
    // crashes if there are entries with address 0.
    u64 filler;
    if (e == begin)
      filler = ctx.etext->get_addr(ctx) - 1;
    else
      filler = e[-1].start;

    for (; e < begin + file->num_areas; e++) {
      e->start = filler;
      e->end = filler;
      e->attr = file->compunits_idx;
    }
  });
}

template <typename E>
CompressedSection<E>::CompressedSection(Context<E> &ctx, Chunk<E> &chunk) {
  assert(chunk.name.starts_with(".debug"));
  this->name = chunk.name;

  uncompressed.reset(new u8[chunk.shdr.sh_size]);
  chunk.write_to(ctx, uncompressed.get());

  switch (ctx.arg.compress_debug_sections) {
  case COMPRESS_ZLIB:
    chdr.ch_type = ELFCOMPRESS_ZLIB;
    compressed.reset(new ZlibCompressor(uncompressed.get(), chunk.shdr.sh_size));
    break;
  case COMPRESS_ZSTD:
    chdr.ch_type = ELFCOMPRESS_ZSTD;
    compressed.reset(new ZstdCompressor(uncompressed.get(), chunk.shdr.sh_size));
    break;
  default:
    unreachable();
  }

  chdr.ch_size = chunk.shdr.sh_size;
  chdr.ch_addralign = chunk.shdr.sh_addralign;

  this->shdr = chunk.shdr;
  this->shdr.sh_flags |= SHF_COMPRESSED;
  this->shdr.sh_addralign = 1;
  this->shdr.sh_size = sizeof(chdr) + compressed->compressed_size;
  this->shndx = chunk.shndx;

  // We don't need to keep the original data unless --gdb-index is given.
  if (!ctx.arg.gdb_index)
    uncompressed.reset(nullptr);
}

template <typename E>
void CompressedSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memcpy(base, &chdr, sizeof(chdr));
  compressed->write_to(base + sizeof(chdr));
}

template <typename E>
RelocSection<E>::RelocSection(Context<E> &ctx, OutputSection<E> &osec)
  : output_section(osec) {
  this->name = save_string(ctx, ".rela" + std::string(osec.name));
  this->shdr.sh_type = SHT_RELA;
  this->shdr.sh_addralign = sizeof(Word<E>);
  this->shdr.sh_entsize = sizeof(RelaTy);

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

  this->shdr.sh_size = num_entries * sizeof(RelaTy);
}

template <typename E>
void RelocSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_link = ctx.symtab->shndx;
  this->shdr.sh_info = output_section.shndx;
}

template <typename E>
static i64 get_output_sym_idx(Symbol<E> &sym) {
  i64 idx2 = sym.file->output_sym_indices[sym.sym_idx];
  assert(idx2 != -1);

  if (sym.sym_idx < sym.file->first_global)
    return sym.file->local_symtab_idx + idx2;
  return sym.file->global_symtab_idx + idx2;
}

template <typename E>
void RelocSection<E>::copy_buf(Context<E> &ctx) {
  tbb::parallel_for((i64)0, (i64)output_section.members.size(), [&](i64 i) {
    RelaTy *buf = (RelaTy *)(ctx.buf + this->shdr.sh_offset) + offsets[i];

    InputSection<E> &isec = *output_section.members[i];
    std::span<const ElfRel<E>> rels = isec.get_rels(ctx);

    for (i64 j = 0; j < rels.size(); j++) {
      const ElfRel<E> &r = rels[j];
      Symbol<E> &sym = *isec.file.symbols[r.r_sym];
      memset(buf + j, 0, sizeof(RelaTy));

      if (sym.esym().st_type != STT_SECTION && !sym.write_to_symtab) {
        buf[j].r_type = R_NONE;
        continue;
      }

      buf[j].r_offset =
        isec.output_section->shdr.sh_addr + isec.offset + r.r_offset;

      if (sym.esym().st_type == STT_SECTION) {
        buf[j].r_type = STT_SECTION;
        buf[j].r_addend = isec.get_addend(r) + isec.offset;

        if (SectionFragment<E> *frag = sym.get_frag())
          buf[j].r_sym = frag->output_section.shndx;
        else
          buf[j].r_sym = sym.get_input_section()->output_section->shndx;
      } else {
        buf[j].r_type = r.r_type;
        buf[j].r_sym = get_output_sym_idx(sym);
        buf[j].r_addend = isec.get_addend(r);
      }
    }
  });
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
template i64 to_phdr_flags(Context<E> &ctx, Chunk<E> *chunk);
template bool is_relro(Context<E> &, Chunk<E> *);
template ElfSym<E> to_output_esym(Context<E> &, Symbol<E> &);

} // namespace mold::elf
