#include "mold.h"

#include <shared_mutex>
#include <sys/mman.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>

#ifdef __APPLE__
#  define COMMON_DIGEST_FOR_OPENSSL
#  include <CommonCrypto/CommonDigest.h>
#  define SHA256(data, len, md) CC_SHA256(data, len, md)
#else
#  include <openssl/sha.h>
#endif

namespace mold::elf {

template <typename E>
void Chunk<E>::write_to(Context<E> &ctx, u8 *buf) {
  Fatal(ctx) << name << ": write_to is called on an invalid section";
}

template <typename E>
u64 get_entry_addr(Context<E> &ctx) {
  if (!ctx.arg.entry.empty())
    if (Symbol<E> *sym = get_symbol(ctx, ctx.arg.entry))
      if (sym->file)
        return sym->get_addr(ctx);

  for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections)
    if (osec->name == ".text")
      return osec->shdr.sh_addr;
  return 0;
}

template <typename E>
void OutputEhdr<E>::copy_buf(Context<E> &ctx) {
  ElfEhdr<E> &hdr = *(ElfEhdr<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(&hdr, 0, sizeof(hdr));

  memcpy(&hdr.e_ident, "\177ELF", 4);
  hdr.e_ident[EI_CLASS] = (E::word_size == 8) ? ELFCLASS64 : ELFCLASS32;
  hdr.e_ident[EI_DATA] = E::is_le ? ELFDATA2LSB : ELFDATA2MSB;
  hdr.e_ident[EI_VERSION] = EV_CURRENT;
  hdr.e_type = ctx.arg.pic ? ET_DYN : ET_EXEC;
  hdr.e_machine = E::e_machine;
  hdr.e_version = EV_CURRENT;
  hdr.e_entry = get_entry_addr(ctx);
  hdr.e_phoff = ctx.phdr->shdr.sh_offset;
  hdr.e_shoff = ctx.shdr->shdr.sh_offset;
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
static i64 to_phdr_flags(Context<E> &ctx, Chunk<E> *chunk) {
  i64 ret = PF_R;
  if (chunk->shdr.sh_flags & SHF_WRITE)
    ret |= PF_W;
  if ((chunk->shdr.sh_flags & SHF_EXECINSTR) ||
      (ctx.arg.z_separate_code == NOSEPARATE_CODE))
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
    if ((flags & SHF_TLS) || type == SHT_INIT_ARRAY ||
        type == SHT_FINI_ARRAY || type == SHT_PREINIT_ARRAY ||
        chunk == ctx.got.get() || chunk == ctx.dynamic.get() ||
        chunk->name.ends_with(".rel.ro"))
      return true;
  return false;
}

template <typename E>
bool separate_page(Context<E> &ctx, Chunk<E> *x, Chunk<E> *y) {
  if (ctx.arg.z_relro && is_relro(ctx, x) != is_relro(ctx, y))
    return true;

  switch (ctx.arg.z_separate_code) {
  case SEPARATE_LOADABLE_SEGMENTS:
    return to_phdr_flags(ctx, x) != to_phdr_flags(ctx, y);
  case SEPARATE_CODE:
    return (x->shdr.sh_flags & SHF_EXECINSTR) != (y->shdr.sh_flags & SHF_EXECINSTR);
  case NOSEPARATE_CODE:
    return false;
  }
  unreachable();
}

template <typename E>
std::vector<ElfPhdr<E>> create_phdr(Context<E> &ctx) {
  std::vector<ElfPhdr<E>> vec;

  auto define = [&](u64 type, u64 flags, i64 min_align, auto &chunk) {
    vec.push_back({});
    ElfPhdr<E> &phdr = vec.back();
    phdr.p_type = type;
    phdr.p_flags = flags;
    phdr.p_align = std::max<u64>(min_align, chunk->shdr.sh_addralign);
    phdr.p_offset = chunk->shdr.sh_offset;
    phdr.p_filesz =
      (chunk->shdr.sh_type == SHT_NOBITS) ? 0 : chunk->shdr.sh_size;
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
  define(PT_PHDR, PF_R, E::word_size, ctx.phdr);

  // Create a PT_INTERP.
  if (ctx.interp)
    define(PT_INTERP, PF_R, 1, ctx.interp);

  // Create a PT_NOTE for each group of SHF_NOTE sections with the same
  // alignment requirement.
  for (i64 i = 0, end = ctx.chunks.size(); i < end;) {
    Chunk<E> *first = ctx.chunks[i++];
    if (!is_note(first))
      continue;

    i64 flags = to_phdr_flags(ctx, first);
    i64 alignment = first->shdr.sh_addralign;
    define(PT_NOTE, flags, alignment, first);

    while (i < end && is_note(ctx.chunks[i]) &&
           to_phdr_flags(ctx, ctx.chunks[i]) == flags &&
           ctx.chunks[i]->shdr.sh_addralign == alignment)
      append(ctx.chunks[i++]);
  }

  // Create PT_LOAD segments.
  {
    std::vector<Chunk<E> *> chunks = ctx.chunks;
    erase(chunks, is_tbss);

    for (i64 i = 0, end = chunks.size(); i < end;) {
      Chunk<E> *first = chunks[i++];
      if (!(first->shdr.sh_flags & SHF_ALLOC))
        break;

      i64 flags = to_phdr_flags(ctx, first);
      define(PT_LOAD, flags, ctx.page_size, first);

      if (!is_bss(first))
        while (i < end && !is_bss(chunks[i]) &&
               to_phdr_flags(ctx, chunks[i]) == flags)
          append(chunks[i++]);

      while (i < end && is_bss(chunks[i]) &&
             to_phdr_flags(ctx, chunks[i]) == flags)
        append(chunks[i++]);
    }
  }

  // Create a PT_TLS.
  for (i64 i = 0; i < ctx.chunks.size(); i++) {
    if (!(ctx.chunks[i]->shdr.sh_flags & SHF_TLS))
      continue;

    define(PT_TLS, to_phdr_flags(ctx, ctx.chunks[i]), 1, ctx.chunks[i]);
    i++;
    while (i < ctx.chunks.size() && (ctx.chunks[i]->shdr.sh_flags & SHF_TLS))
      append(ctx.chunks[i++]);
  }

  // Add PT_DYNAMIC
  if (ctx.dynamic->shdr.sh_size)
    define(PT_DYNAMIC, PF_R | PF_W, 1, ctx.dynamic);

  // Add PT_GNU_EH_FRAME
  if (ctx.eh_frame_hdr)
    define(PT_GNU_EH_FRAME, PF_R, 1, ctx.eh_frame_hdr);

  // Add PT_GNU_STACK, which is a marker segment that doesn't really
  // contain any segments. It controls executable bit of stack area.
  ElfPhdr<E> phdr = {};
  phdr.p_type = PT_GNU_STACK,
  phdr.p_flags = ctx.arg.z_execstack ? (PF_R | PF_W | PF_X) : (PF_R | PF_W),
  vec.push_back(phdr);

  // Create a PT_GNU_RELRO.
  if (ctx.arg.z_relro) {
    for (i64 i = 0; i < ctx.chunks.size(); i++) {
      if (!is_relro(ctx, ctx.chunks[i]))
        continue;

      define(PT_GNU_RELRO, PF_R, 1, ctx.chunks[i]);
      i++;
      while (i < ctx.chunks.size() && is_relro(ctx, ctx.chunks[i]))
        append(ctx.chunks[i++]);
    }
  }

  return vec;
}

template <typename E>
void OutputPhdr<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = create_phdr(ctx).size() * sizeof(ElfPhdr<E>);
}

template <typename E>
void OutputPhdr<E>::copy_buf(Context<E> &ctx) {
  std::vector<ElfPhdr<E>> vec = create_phdr(ctx);
  assert(this->shdr.sh_size == vec.size() * sizeof(ElfPhdr<E>));
  write_vector(ctx.buf + this->shdr.sh_offset, vec);
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

  offset += ctx.dynbss->symbols.size() * sizeof(ElfRel<E>);
  offset += ctx.dynbss_relro->symbols.size() * sizeof(ElfRel<E>);

  for (ObjectFile<E> *file : ctx.objs) {
    file->reldyn_offset = offset;
    offset += file->num_dynrel * sizeof(ElfRel<E>);
  }
  this->shdr.sh_size = offset;
}

template <typename E>
static ElfRel<E> reloc(u64 offset, u32 type, u32 sym, i64 addend = 0);

template <>
ElfRel<X86_64> reloc<X86_64>(u64 offset, u32 type, u32 sym, i64 addend) {
  return {offset, type, sym, addend};
}

template <>
ElfRel<I386> reloc<I386>(u64 offset, u32 type, u32 sym, i64 addend) {
  return {(u32)offset, type, sym};
}

template <>
ElfRel<ARM64> reloc<ARM64>(u64 offset, u32 type, u32 sym, i64 addend) {
  return {offset, type, sym, addend};
}

template <typename E>
void RelDynSection<E>::copy_buf(Context<E> &ctx) {
  ElfRel<E> *rel = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset +
                                 ctx.got->get_reldyn_size(ctx));

  for (Symbol<E> *sym : ctx.dynbss->symbols)
    *rel++ = reloc<E>(sym->get_addr(ctx), E::R_COPY, sym->get_dynsym_idx(ctx));

  for (Symbol<E> *sym : ctx.dynbss_relro->symbols)
    *rel++ = reloc<E>(sym->get_addr(ctx), E::R_COPY, sym->get_dynsym_idx(ctx));
}

template <typename E>
void RelDynSection<E>::sort(Context<E> &ctx) {
  Timer t(ctx, "sort_dynamic_relocs");

  ElfRel<E> *begin = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset);
  ElfRel<E> *end = (ElfRel<E> *)((u8 *)begin + this->shdr.sh_size);

  tbb::parallel_sort(begin, end, [](const ElfRel<E> &a, const ElfRel<E> &b) {
    return std::tuple(a.r_type != E::R_RELATIVE, a.r_sym, a.r_offset) <
           std::tuple(b.r_type != E::R_RELATIVE, b.r_sym, b.r_offset);
  });

  // Dynamic section contains the number of R_RELATIVE dynamic relocations,
  // so rewrite it if necessary.
  if (ctx.dynamic->shdr.sh_size) {
    auto it = std::find_if(begin, end, [](const ElfRel<E> &rel) {
      return rel.r_type != E::R_RELATIVE;
    });
    this->relcount = it - begin;
    ctx.dynamic->copy_buf(ctx);
  }
}

template <typename E>
void StrtabSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = 1;
  for (ObjectFile<E> *file : ctx.objs) {
    file->strtab_offset = this->shdr.sh_size;
    this->shdr.sh_size += file->strtab_size;
  }
}

template <typename E>
void ShstrtabSection<E>::update_shdr(Context<E> &ctx) {
  std::unordered_map<std::string_view, i64> map;
  i64 offset = 1;

  for (Chunk<E> *chunk : ctx.chunks)
    if (!chunk->name.empty() && map.insert({chunk->name, offset}).second)
      offset += chunk->name.size() + 1;

  this->shdr.sh_size = offset;

  for (Chunk<E> *chunk : ctx.chunks)
    if (!chunk->name.empty())
      chunk->shdr.sh_name = map[chunk->name];
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
  auto [it, inserted] = strings.insert({str, this->shdr.sh_size});
  if (inserted)
    this->shdr.sh_size += str.size() + 1;
  return it->second;
}

template <typename E>
i64 DynstrSection<E>::find_string(std::string_view str) {
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
    for (Symbol<E> *sym :
           std::span<Symbol<E> *>(ctx.dynsym->symbols).subspan(1)) {
      write_string(base + offset, sym->name());
      offset += sym->name().size() + 1;
    }
  }
}

template <typename E>
void SymtabSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = sizeof(ElfSym<E>);

  for (ObjectFile<E> *file : ctx.objs) {
    file->local_symtab_offset = this->shdr.sh_size;
    this->shdr.sh_size += file->num_local_symtab * sizeof(ElfSym<E>);
  }

  for (ObjectFile<E> *file : ctx.objs) {
    file->global_symtab_offset = this->shdr.sh_size;
    this->shdr.sh_size += file->num_global_symtab * sizeof(ElfSym<E>);
  }

  this->shdr.sh_info = ctx.objs[0]->global_symtab_offset / sizeof(ElfSym<E>);
  this->shdr.sh_link = ctx.strtab->shndx;

  if (this->shdr.sh_size == sizeof(ElfSym<E>))
    this->shdr.sh_size = 0;

  static Counter counter("symtab");
  counter += this->shdr.sh_size / sizeof(ElfSym<E>);
}

template <typename E>
void SymtabSection<E>::copy_buf(Context<E> &ctx) {
  memset(ctx.buf + this->shdr.sh_offset, 0, sizeof(ElfSym<E>));
  ctx.buf[ctx.strtab->shdr.sh_offset] = '\0';

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->write_symtab(ctx);
  });
}

template <typename E>
static bool has_init_array(Context<E> &ctx) {
  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->shdr.sh_type == SHT_INIT_ARRAY)
      return true;
  return false;
}

template <typename E>
static bool has_fini_array(Context<E> &ctx) {
  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->shdr.sh_type == SHT_FINI_ARRAY)
      return true;
  return false;
}

template <typename E>
static std::vector<typename E::WordTy> create_dynamic_section(Context<E> &ctx) {
  std::vector<typename E::WordTy> vec;

  auto define = [&](u64 tag, u64 val) {
    vec.push_back(tag);
    vec.push_back(val);
  };

  for (SharedFile<E> *file : ctx.dsos)
    define(DT_NEEDED, ctx.dynstr->find_string(file->soname));

  if (!ctx.arg.rpaths.empty())
    define(DT_RUNPATH, ctx.dynstr->find_string(ctx.arg.rpaths));

  if (!ctx.arg.soname.empty())
    define(DT_SONAME, ctx.dynstr->find_string(ctx.arg.soname));

  for (std::string_view str : ctx.arg.auxiliary)
    define(DT_AUXILIARY, ctx.dynstr->find_string(str));

  for (std::string_view str : ctx.arg.filter)
    define(DT_FILTER, ctx.dynstr->find_string(str));

  if (ctx.reldyn->shdr.sh_size) {
    define(E::is_rel ? DT_REL : DT_RELA, ctx.reldyn->shdr.sh_addr);
    define(E::is_rel ? DT_RELSZ : DT_RELASZ, ctx.reldyn->shdr.sh_size);
    define(E::is_rel ? DT_RELENT : DT_RELAENT, sizeof(ElfRel<E>));
  }

  if (ctx.relplt->shdr.sh_size) {
    define(DT_JMPREL, ctx.relplt->shdr.sh_addr);
    define(DT_PLTRELSZ, ctx.relplt->shdr.sh_size);
    define(DT_PLTREL, E::is_rel ? DT_REL : DT_RELA);
  }

  if (ctx.gotplt->shdr.sh_size)
    define(DT_PLTGOT, ctx.gotplt->shdr.sh_addr);

  if (ctx.dynsym->shdr.sh_size) {
    define(DT_SYMTAB, ctx.dynsym->shdr.sh_addr);
    define(DT_SYMENT, sizeof(ElfSym<E>));
  }

  if (ctx.dynstr->shdr.sh_size) {
    define(DT_STRTAB, ctx.dynstr->shdr.sh_addr);
    define(DT_STRSZ, ctx.dynstr->shdr.sh_size);
  }

  if (has_init_array(ctx)) {
    define(DT_INIT_ARRAY, ctx.__init_array_start->value);
    define(DT_INIT_ARRAYSZ,
           ctx.__init_array_end->value - ctx.__init_array_start->value);
  }

  if (has_fini_array(ctx)) {
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

  if (Symbol<E> *sym = get_symbol(ctx, ctx.arg.init); sym->file)
    define(DT_INIT, sym->get_addr(ctx));
  if (Symbol<E> *sym = get_symbol(ctx, ctx.arg.fini); sym->file)
    define(DT_FINI, sym->get_addr(ctx));

  if (ctx.hash)
    define(DT_HASH, ctx.hash->shdr.sh_addr);
  if (ctx.gnu_hash)
    define(DT_GNU_HASH, ctx.gnu_hash->shdr.sh_addr);
  if (ctx.reldyn)
    define(E::is_rel ? DT_RELCOUNT : DT_RELACOUNT, ctx.reldyn->relcount);
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

  define(DT_DEBUG, 0);
  define(DT_NULL, 0);

  for (i64 i = 0; i < ctx.arg.spare_dynamic_tags; i++)
    define(DT_NULL, 0);

  return vec;
}

template <typename E>
void DynamicSection<E>::update_shdr(Context<E> &ctx) {
  if (ctx.arg.is_static)
    return;
  if (!ctx.arg.pic && ctx.dsos.empty())
    return;

  this->shdr.sh_size = create_dynamic_section(ctx).size() * E::word_size;
  this->shdr.sh_link = ctx.dynstr->shndx;
}

template <typename E>
void DynamicSection<E>::copy_buf(Context<E> &ctx) {
  std::vector<typename E::WordTy> contents = create_dynamic_section(ctx);
  assert(this->shdr.sh_size == contents.size() * sizeof(contents[0]));
  write_vector(ctx.buf + this->shdr.sh_offset, contents);
}

template <typename E>
static std::string_view get_output_name(Context<E> &ctx, std::string_view name) {
  if (ctx.arg.unique &&
      std::regex_match(name.begin(), name.end(), *ctx.arg.unique))
    return name;

  if (ctx.arg.z_keep_text_section_prefix) {
    static std::string_view text_prefixes[] = {
      ".text.hot.", ".text.unknown.", ".text.unlikely.", ".text.startup.",
      ".text.exit."
    };

    for (std::string_view prefix : text_prefixes) {
      std::string_view stem = prefix.substr(0, prefix.size() - 1);
      if (name == stem || name.starts_with(prefix))
        return stem;
    }
  }

  if (name.starts_with( ".rodata.cst"))
    return ".rodata.cst";
  if (name.starts_with( ".rodata.str"))
    return ".rodata.str";

  static std::string_view prefixes[] = {
    ".text.", ".data.rel.ro.", ".data.", ".rodata.", ".bss.rel.ro.", ".bss.",
    ".init_array.", ".fini_array.", ".tbss.", ".tdata.", ".gcc_except_table.",
  };

  for (std::string_view prefix : prefixes) {
    std::string_view stem = prefix.substr(0, prefix.size() - 1);
    if (name == stem || name.starts_with(prefix))
      return stem;
  }

  if (name.starts_with(".zdebug_")) {
    if (name == ".zdebug_aranges")
      return ".debug_aranges";
    if (name == ".zdebug_frame")
      return ".debug_frame";
    if (name == ".zdebug_info")
      return ".debug_info";
    if (name == ".zdebug_line")
      return ".debug_line";
    if (name == ".zdebug_loc")
      return ".debug_loc";
    if (name == ".zdebug_pubnames")
      return ".debug_pubnames";
    if (name == ".zdebug_pubtypes")
      return ".debug_pubtypes";
    if (name == ".zdebug_ranges")
      return ".debug_ranges";
    if (name == ".zdebug_str")
      return ".debug_str";
    if (name == ".zdebug_types")
      return ".debug_types";
    return save_string(ctx, "."s + std::string(name.substr(2)));
  }

  return name;
}

template <typename E>
OutputSection<E>::OutputSection(std::string_view name, u32 type,
                                u64 flags, u32 idx)
  : Chunk<E>(Chunk<E>::REGULAR), idx(idx) {
  this->name = name;
  this->shdr.sh_type = type;
  this->shdr.sh_flags = flags;
}

static u64 canonicalize_type(std::string_view name, u64 type) {
  if (type == SHT_PROGBITS) {
    if (name == ".init_array" || name.starts_with(".init_array."))
      return SHT_INIT_ARRAY;
    if (name == ".fini_array" || name.starts_with(".fini_array."))
      return SHT_FINI_ARRAY;
  }
  if (type == SHT_X86_64_UNWIND)
    return SHT_PROGBITS;
  return type;
}

template <typename E>
OutputSection<E> *
OutputSection<E>::get_instance(Context<E> &ctx, std::string_view name,
                               u64 type, u64 flags) {
  name = get_output_name(ctx, name);
  type = canonicalize_type(name, type);
  flags = flags & ~(u64)SHF_GROUP & ~(u64)SHF_COMPRESSED;

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
  ctx.output_sections.push_back(std::unique_ptr<OutputSection<E>>(osec));
  return osec;
}

template <typename E>
void OutputSection<E>::copy_buf(Context<E> &ctx) {
  if (this->shdr.sh_type != SHT_NOBITS)
    write_to(ctx, ctx.buf + this->shdr.sh_offset);
}

template <typename E>
void OutputSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  tbb::parallel_for((i64)0, (i64)members.size(), [&](i64 i) {
    // Copy section contents to an output file
    InputSection<E> &isec = *members[i];
    isec.write_to(ctx, buf + isec.offset);

    // Zero-clear trailing padding
    u64 this_end = isec.offset + isec.shdr.sh_size;
    u64 next_start = (i == members.size() - 1) ?
      this->shdr.sh_size : members[i + 1]->offset;
    memset(buf + this_end, 0, next_start - this_end);
  });
}

template <typename E>
void GotSection<E>::add_got_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_got_idx(ctx, this->shdr.sh_size / E::word_size);
  this->shdr.sh_size += E::word_size;
  got_syms.push_back(sym);
}

template <typename E>
void GotSection<E>::add_gottp_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_gottp_idx(ctx, this->shdr.sh_size / E::word_size);
  this->shdr.sh_size += E::word_size;
  gottp_syms.push_back(sym);
}

template <typename E>
void GotSection<E>::add_tlsgd_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_tlsgd_idx(ctx, this->shdr.sh_size / E::word_size);
  this->shdr.sh_size += E::word_size * 2;
  tlsgd_syms.push_back(sym);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsdesc_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_tlsdesc_idx(ctx, this->shdr.sh_size / E::word_size);
  this->shdr.sh_size += E::word_size * 2;
  tlsdesc_syms.push_back(sym);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsld(Context<E> &ctx) {
  if (tlsld_idx != -1)
    return;
  tlsld_idx = this->shdr.sh_size / E::word_size;
  this->shdr.sh_size += E::word_size * 2;
}

template <typename E>
u64 GotSection<E>::get_tlsld_addr(Context<E> &ctx) const {
  assert(tlsld_idx != -1);
  return this->shdr.sh_addr + tlsld_idx * E::word_size;
}

template <typename E>
i64 GotSection<E>::get_reldyn_size(Context<E> &ctx) const {
  i64 n = 0;
  for (Symbol<E> *sym : got_syms)
    if (sym->is_imported || (ctx.arg.pic && sym->is_relative()) ||
        sym->get_type() == STT_GNU_IFUNC)
      n++;

  n += tlsgd_syms.size() * 2;
  n += tlsdesc_syms.size();

  for (Symbol<E> *sym : gottp_syms)
    if (sym->is_imported || ctx.arg.shared)
      n++;

  if (tlsld_idx != -1)
    n++;

  return n * sizeof(ElfRel<E>);
}

// Fill .got and .rel.dyn.
template <typename E>
void GotSection<E>::copy_buf(Context<E> &ctx) {
  typename E::WordTy *buf =
    (typename E::WordTy *)(ctx.buf + this->shdr.sh_offset);

  memset(buf, 0, this->shdr.sh_size);

  ElfRel<E> *rel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset);

  for (Symbol<E> *sym : got_syms) {
    u64 addr = sym->get_got_addr(ctx);
    if (sym->is_imported) {
      *rel++ = reloc<E>(addr, E::R_GLOB_DAT, sym->get_dynsym_idx(ctx));
    } else if (sym->get_type() == STT_GNU_IFUNC) {
      u64 resolver_addr = sym->input_section->get_addr() + sym->value;
      *rel++ = reloc<E>(addr, E::R_IRELATIVE, 0, resolver_addr);
      if (E::is_rel)
        buf[sym->get_got_idx(ctx)] = resolver_addr;
    } else {
      buf[sym->get_got_idx(ctx)] = sym->get_addr(ctx);
      if (ctx.arg.pic && sym->is_relative())
        *rel++ = reloc<E>(addr, E::R_RELATIVE, 0, (i64)sym->get_addr(ctx));
    }
  }

  for (Symbol<E> *sym : tlsgd_syms) {
    u64 addr = sym->get_tlsgd_addr(ctx);
    i32 dynsym_idx = sym->get_dynsym_idx(ctx);
    *rel++ = reloc<E>(addr, E::R_DTPMOD, dynsym_idx);
    *rel++ = reloc<E>(addr + E::word_size, E::R_DTPOFF, dynsym_idx);
  }

  for (Symbol<E> *sym : tlsdesc_syms)
    *rel++ = reloc<E>(sym->get_tlsdesc_addr(ctx), E::R_TLSDESC,
                      sym->get_dynsym_idx(ctx));

  for (Symbol<E> *sym : gottp_syms) {
    if (sym->is_imported) {
      *rel++ = reloc<E>(sym->get_gottp_addr(ctx), E::R_TPOFF,
                        sym->get_dynsym_idx(ctx));
    } else if (ctx.arg.shared) {
      *rel++ = reloc<E>(sym->get_gottp_addr(ctx), E::R_TPOFF, 0,
                        sym->get_addr(ctx) - ctx.tls_begin);
    } else if (E::e_machine == EM_386 || E::e_machine == EM_X86_64) {
      buf[sym->get_gottp_idx(ctx)] = sym->get_addr(ctx) - ctx.tls_end;
    } else {
      assert(E::e_machine == EM_AARCH64);
      buf[sym->get_gottp_idx(ctx)] = sym->get_addr(ctx) - ctx.tls_begin + 16;
    }
  }

  if (tlsld_idx != -1)
    *rel++ = reloc<E>(get_tlsld_addr(ctx), E::R_DTPMOD, 0);
}

template <typename E>
void PltSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(!sym->has_plt(ctx));

  if (this->shdr.sh_size == 0) {
    this->shdr.sh_size = ctx.plt_hdr_size;
    ctx.gotplt->shdr.sh_size = E::word_size * 3;
  }

  sym->set_plt_idx(ctx, symbols.size());
  this->shdr.sh_size += ctx.plt_size;
  symbols.push_back(sym);

  sym->set_gotplt_idx(ctx, ctx.gotplt->shdr.sh_size / E::word_size);
  ctx.gotplt->shdr.sh_size += E::word_size;
  ctx.relplt->shdr.sh_size += sizeof(ElfRel<E>);
  ctx.dynsym->add_symbol(ctx, sym);
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
void RelPltSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_link = ctx.dynsym->shndx;
  this->shdr.sh_info = ctx.gotplt->shndx;
}

template <typename E>
void RelPltSection<E>::copy_buf(Context<E> &ctx) {
  ElfRel<E> *buf = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset);

  i64 relplt_idx = 0;
  for (Symbol<E> *sym : ctx.plt->symbols)
    buf[relplt_idx++] = reloc<E>(sym->get_gotplt_addr(ctx), E::R_JUMP_SLOT,
                                 sym->get_dynsym_idx(ctx));
}

template <typename E>
void DynsymSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  if (sym->get_dynsym_idx(ctx) != -1)
    return;
  sym->set_dynsym_idx(ctx, -2);
  symbols.push_back(sym);
}

template <typename E>
void DynsymSection<E>::finalize(Context<E> &ctx) {
  Timer t(ctx, "DynsymSection::finalize");

  auto is_local = [](Symbol<E> *sym) {
    return !sym->is_imported && !sym->is_exported;
  };

  // In any symtab, local symbols must precede global symbols.
  // We also place undefined symbols before defined symbols for .gnu.hash.
  sort(symbols.begin() + 1, symbols.end(), [&](Symbol<E> *a, Symbol<E> *b) {
    if (auto x = (is_local(a) <=> is_local(b)); x != 0)
      return x > 0;
    return a->is_exported < b->is_exported;
  });

  auto first_global = std::partition_point(symbols.begin() + 1, symbols.end(),
                                           is_local);

  // If we have .gnu.hash section, we need to sort .dynsym contents by
  // symbol hashes.
  if (ctx.gnu_hash) {
    // We need a stable sort for build reproducibility, but parallel_sort
    // isn't stable, so we use this struct to make it stable.
    struct T {
      Symbol<E> *sym;
      u32 hash;
      i32 idx;
    };

    auto first_exported = std::partition_point(
      first_global, symbols.end(), [](Symbol<E> *x) { return !x->is_exported; });

    i64 base_offset = first_exported - symbols.begin();
    i64 num_exported = symbols.end() - first_exported;

    std::vector<T> vec(num_exported);
    ctx.gnu_hash->num_buckets = num_exported / ctx.gnu_hash->LOAD_FACTOR + 1;

    tbb::parallel_for((i64)0, num_exported, [&](i64 i) {
      Symbol<E> *sym = symbols[base_offset + i];
      vec[i].sym = sym;
      vec[i].hash = djb_hash(sym->name()) % ctx.gnu_hash->num_buckets;
      vec[i].idx = i;
    });

    tbb::parallel_sort(vec.begin(), vec.end(), [&](const T &a, const T &b) {
      return std::tuple(a.hash, a.idx) < std::tuple(b.hash, b.idx);
    });

    for (i64 i = 0; i < num_exported; i++)
      symbols[base_offset + i] = vec[i].sym;
  }

  ctx.dynstr->dynsym_offset = ctx.dynstr->shdr.sh_size;

  for (i64 i = 1; i < symbols.size(); i++) {
    symbols[i]->set_dynsym_idx(ctx, i);
    ctx.dynstr->shdr.sh_size += symbols[i]->name().size() + 1;
  }

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

    memset(&esym, 0, sizeof(esym));
    esym.st_type = sym.esym().st_type;
    esym.st_size = sym.esym().st_size;

    if (i < this->shdr.sh_info)
      esym.st_bind = STB_LOCAL;
    else if (sym.is_weak)
      esym.st_bind = STB_WEAK;
    else if (sym.file->is_dso)
      esym.st_bind = STB_GLOBAL;
    else
      esym.st_bind = sym.esym().st_bind;

    esym.st_name = name_offset;
    name_offset += sym.name().size() + 1;

    if (sym.has_copyrel) {
      esym.st_shndx = sym.copyrel_readonly
        ? ctx.dynbss_relro->shndx : ctx.dynbss->shndx;
      esym.st_value = sym.get_addr(ctx);
    } else if (sym.file->is_dso || sym.esym().is_undef()) {
      esym.st_shndx = SHN_UNDEF;
      esym.st_size = 0;
      if (sym.has_plt(ctx) && !ctx.arg.pic && sym.is_imported) {
        // Emit an address for a canonical PLT
        esym.st_value = sym.get_plt_addr(ctx);
      }
    } else if (!sym.input_section) {
      esym.st_shndx = SHN_ABS;
      esym.st_value = sym.get_addr(ctx);
    } else if (sym.get_type() == STT_TLS) {
      esym.st_shndx = sym.input_section->output_section->shndx;
      esym.st_value = sym.get_addr(ctx) - ctx.tls_begin;
    } else {
      esym.st_shndx = sym.input_section->output_section->shndx;
      esym.st_value = sym.get_addr(ctx, false);
      esym.st_visibility = sym.visibility;
    }
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
  u32 *hdr = (u32 *)base;
  u32 *buckets = (u32 *)(base + 8);
  u32 *chains = buckets + num_slots;

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
    num_bloom = next_power_of_two(num_bits / ELFCLASS_BITS);
  }

  this->shdr.sh_size = HEADER_SIZE;               // Header
  this->shdr.sh_size += num_bloom * E::word_size; // Bloom filter
  this->shdr.sh_size += num_buckets * 4;          // Hash buckets
  this->shdr.sh_size += num_exported * 4;         // Hash values
}

template <typename E>
void GnuHashSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memset(base, 0, this->shdr.sh_size);

  std::span<Symbol<E> *> syms = get_exported_symbols(ctx);
  i64 exported_offset = ctx.dynsym->symbols.size() - syms.size();

  *(u32 *)base = num_buckets;
  *(u32 *)(base + 4) = exported_offset;
  *(u32 *)(base + 8) = num_bloom;
  *(u32 *)(base + 12) = BLOOM_SHIFT;

  std::vector<u32> hashes(syms.size());
  for (i64 i = 0; i < syms.size(); i++)
    hashes[i] = djb_hash(syms[i]->name());

  // Write a bloom filter
  typename E::WordTy *bloom = (typename E::WordTy *)(base + HEADER_SIZE);
  for (i64 hash : hashes) {
    i64 idx = (hash / ELFCLASS_BITS) % num_bloom;
    bloom[idx] |= (u64)1 << (hash % ELFCLASS_BITS);
    bloom[idx] |= (u64)1 << ((hash >> BLOOM_SHIFT) % ELFCLASS_BITS);
  }

  // Write hash bucket indices
  u32 *buckets = (u32 *)(bloom + num_bloom);
  for (i64 i = 0; i < hashes.size(); i++) {
    i64 idx = hashes[i] % num_buckets;
    if (!buckets[idx])
      buckets[idx] = i + exported_offset;
  }

  // Write a hash table
  u32 *table = buckets + num_buckets;
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
MergedSection<E>::MergedSection(std::string_view name, u64 flags, u32 type)
  : Chunk<E>(this->SYNTHETIC) {
  this->name = name;
  this->shdr.sh_flags = flags;
  this->shdr.sh_type = type;
}

template <typename E>
MergedSection<E> *
MergedSection<E>::get_instance(Context<E> &ctx, std::string_view name,
                               u64 type, u64 flags) {
  name = get_output_name(ctx, name);
  flags = flags & ~(u64)SHF_MERGE & ~(u64)SHF_STRINGS;

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

  auto *osec = new MergedSection(name, flags, type);
  ctx.merged_sections.push_back(std::unique_ptr<MergedSection>(osec));
  return osec;
}

template <typename E>
SectionFragment<E> *
MergedSection<E>::insert(std::string_view data, u64 hash, i64 alignment) {
  assert(alignment < UINT16_MAX);

  std::call_once(once_flag, [&]() {
    // We aim 2/3 occupation ratio
    map.resize(estimator.get_cardinality() * 3 / 2);
  });

  SectionFragment<E> *frag;
  bool inserted;
  std::tie(frag, inserted) = map.insert(data, hash, SectionFragment(this));
  assert(frag);

  for (u16 cur = frag->alignment; cur < alignment;)
    if (frag->alignment.compare_exchange_weak(cur, alignment))
      break;
  return frag;
}

template <typename E>
void MergedSection<E>::assign_offsets(Context<E> &ctx) {
  std::vector<i64> sizes(map.NUM_SHARDS);
  std::vector<i64> max_alignments(map.NUM_SHARDS);
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
        fragments.push_back({{map.keys[j], map.sizes[j]}, &frag});

    // Sort fragments to make output deterministic.
    tbb::parallel_sort(fragments.begin(), fragments.end(),
                       [](const KeyVal &a, const KeyVal &b) {
      if (a.val->alignment != b.val->alignment)
        return a.val->alignment < b.val->alignment;
      if (a.key.size() != b.key.size())
        return a.key.size() < b.key.size();
      return a.key < b.key;
    });

    // Assign offsets.
    i64 offset = 0;
    i64 max_alignment = 0;

    for (KeyVal &kv : fragments) {
      SectionFragment<E> &frag = *kv.val;
      offset = align_to(offset, frag.alignment);
      frag.offset = offset;
      offset += kv.key.size();
      max_alignment = std::max<i64>(max_alignment, frag.alignment);
    }

    sizes[i] = offset;
    max_alignments[i] = max_alignment;

    static Counter merged_strings("merged_strings");
    merged_strings += fragments.size();
  });

  i64 alignment = 1;
  for (i64 x : max_alignments)
    alignment = std::max(alignment, x);

  for (i64 i = 1; i < map.NUM_SHARDS + 1; i++)
    shard_offsets[i] =
      align_to(shard_offsets[i - 1] + sizes[i - 1], alignment);

  tbb::parallel_for((i64)1, map.NUM_SHARDS, [&](i64 i) {
    for (i64 j = shard_size * i; j < shard_size * (i + 1); j++)
      if (SectionFragment<E> &frag = map.values[j]; frag.is_alive)
        frag.offset += shard_offsets[i];
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
    memset(buf + shard_offsets[i], 0, shard_offsets[i + 1] - shard_offsets[i]);

    for (i64 j = shard_size * i; j < shard_size * (i + 1); j++)
      if (SectionFragment<E> &frag = map.values[j]; frag.is_alive)
        memcpy(buf + frag.offset, map.keys[j], map.sizes[j]);
  });
}

template <typename E>
void EhFrameSection<E>::construct(Context<E> &ctx) {
  // Remove dead FDEs and assign them offsets within their corresponding
  // CIE group.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    erase(file->fdes, [](FdeRecord<E> &fde) { return !fde.is_alive; });

    i64 offset = 0;
    for (FdeRecord<E> &fde : file->fdes) {
      fde.output_offset = offset;
      offset += fde.size();
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

template <typename E>
void EhFrameSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    // Copy CIEs.
    for (CieRecord<E> &cie : file->cies) {
      if (!cie.is_leader)
        continue;

      std::string_view contents = cie.get_contents();
      memcpy(base + cie.output_offset, contents.data(), contents.size());

      for (ElfRel<E> &rel : cie.get_rels()) {
        if (rel.r_type == E::R_NONE)
          continue;
        assert(rel.r_offset - cie.input_offset < contents.size());
        u64 loc = cie.output_offset + rel.r_offset - cie.input_offset;
        u64 val = file->symbols[rel.r_sym]->get_addr(ctx);
        u64 addend = cie.input_section.get_addend(rel);
        apply_reloc(ctx, rel, loc, val + addend);
      }
    }

    // Copy FDEs.
    for (FdeRecord<E> &fde : file->fdes) {
      i64 offset = file->fde_offset + fde.output_offset;

      std::string_view contents = fde.get_contents();
      memcpy(base + offset, contents.data(), contents.size());

      *(u32 *)(base + offset + 4) = offset + 4 - fde.cie->output_offset;

      for (ElfRel<E> &rel : fde.get_rels()) {
        if (rel.r_type == E::R_NONE)
          continue;
        assert(rel.r_offset - fde.input_offset < contents.size());
        u64 loc = offset + rel.r_offset - fde.input_offset;
        u64 val = file->symbols[rel.r_sym]->get_addr(ctx);
        u64 addend = fde.cie->input_section.get_addend(rel);
        apply_reloc(ctx, rel, loc, val + addend);
      }
    }
  });

  // Write a terminator.
  *(u32 *)(base + this->shdr.sh_size - 4) = 0;
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
  u64 eh_frame_addr = ctx.eh_frame->shdr.sh_addr;

  // Write a header
  base[0] = 1;
  base[1] = DW_EH_PE_pcrel | DW_EH_PE_sdata4;
  base[2] = DW_EH_PE_udata4;
  base[3] = DW_EH_PE_datarel | DW_EH_PE_sdata4;

  *(u32 *)(base + 4) = eh_frame_addr - this->shdr.sh_addr - 4;
  *(u32 *)(base + 8) = num_fdes;

  // Fill contents
  struct Entry {
    i32 init_addr;
    i32 fde_addr;
  };

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    Entry *entries = (Entry *)(base + HEADER_SIZE) + file->fde_idx;

    for (i64 i = 0; i < file->fdes.size(); i++) {
      FdeRecord<E> &fde = file->fdes[i];

      ElfRel<E> &rel = fde.cie->rels[fde.rel_idx];
      u64 val = file->symbols[rel.r_sym]->get_addr(ctx);
      u64 addend = fde.cie->input_section.get_addend(rel);
      i64 offset = file->fde_offset + fde.output_offset;

      entries[i].init_addr = val + addend - this->shdr.sh_addr;
      entries[i].fde_addr = eh_frame_addr + offset - this->shdr.sh_addr;
    }
  });

  // Sort contents
  Entry *begin = (Entry *)(base + HEADER_SIZE);
  Entry *end = begin + num_fdes;

  tbb::parallel_sort(begin, end, [](const Entry &a, const Entry &b) {
    return a.init_addr < b.init_addr;
  });
}

template <typename E>
void DynbssSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
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

  erase(syms, [](Symbol<E> *sym) {
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
  contents.resize((sizeof(ElfVerneed<E>) + sizeof(ElfVernaux<E>)) *
                  syms.size());

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

template <typename E>
i64 BuildId::size(Context<E> &ctx) const {
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
  this->shdr.sh_size = HEADER_SIZE + ctx.arg.build_id.size(ctx);
}

template <typename E>
void BuildIdSection<E>::copy_buf(Context<E> &ctx) {
  u32 *base = (u32 *)(ctx.buf + this->shdr.sh_offset);
  memset(base, 0, this->shdr.sh_size);
  base[0] = 4;                          // Name size
  base[1] = ctx.arg.build_id.size(ctx); // Hash size
  base[2] = NT_GNU_BUILD_ID;            // Type
  memcpy(base + 3, "GNU", 4);           // Name string
}

template <typename E>
static void compute_sha256(Context<E> &ctx, i64 offset) {
  u8 *buf = ctx.buf;
  i64 bufsize = ctx.output_file->filesize;

  i64 shard_size = 4096 * 1024;
  i64 num_shards = bufsize / shard_size + 1;
  std::vector<u8> shards(num_shards * SHA256_SIZE);

  tbb::parallel_for((i64)0, num_shards, [&](i64 i) {
    u8 *begin = buf + shard_size * i;
    i64 sz = (i < num_shards - 1) ? shard_size : (bufsize % shard_size);
    SHA256(begin, sz, shards.data() + i * SHA256_SIZE);

    // We call munmap early for each chunk so that the last munmap
    // gets cheaper. We assume that the .note.build-id section is
    // at the beginning of an output file. This is an ugly performance
    // hack, but we can save about 30 ms for a 2 GiB output.
    if (i > 0 && ctx.output_file->is_mmapped)
      munmap(begin, sz);
  });

  assert(ctx.arg.build_id.size(ctx) <= SHA256_SIZE);

  u8 digest[SHA256_SIZE];
  SHA256(shards.data(), shards.size(), digest);
  memcpy(buf + offset, digest, ctx.arg.build_id.size(ctx));

  if (ctx.output_file->is_mmapped) {
    munmap(buf, std::min(bufsize, shard_size));
    ctx.output_file->is_unmapped = true;
  }
}

template <typename E>
static std::vector<u8> get_uuid_v4(Context<E> &ctx) {
  std::vector<u8> buf(16);

  FILE *fp = fopen("/dev/urandom", "r");
  if (!fp)
    Fatal(ctx) << "cannot open /dev/urandom: " << errno_string();
  if (fread(buf.data(), buf.size(), 1, fp) != 1)
    Fatal(ctx) << "fread on /dev/urandom: short read";
  fclose(fp);

  // Indicate that this is UUIDv4.
  buf[6] &= 0b00001111;
  buf[6] |= 0b01000000;

  // Indicates that this is an RFC4122 variant.
  buf[8] &= 0b00111111;
  buf[8] |= 0b10000000;
  return buf;
}

template <typename E>
void BuildIdSection<E>::write_buildid(Context<E> &ctx) {
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
  case BuildId::UUID:
    write_vector(ctx.buf + this->shdr.sh_offset + HEADER_SIZE,
                 get_uuid_v4(ctx));
    return;
  default:
    unreachable();
  }
}

template <typename E>
void NotePropertySection<E>::update_shdr(Context<E> &ctx) {
  features = -1;
  for (ObjectFile<E> *file : ctx.objs)
    features &= file->features;

  if (ctx.arg.z_shstk)
    features |= GNU_PROPERTY_X86_FEATURE_1_SHSTK;

  if (features != 0 && features != -1)
    this->shdr.sh_size = (E::word_size == 8) ? 32 : 28;
}

template <typename E>
void NotePropertySection<E>::copy_buf(Context<E> &ctx) {
  u32 *buf = (u32 *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  buf[0] = 4;                              // Name size
  buf[1] = (E::word_size == 8) ? 16 : 12;   // Content size
  buf[2] = NT_GNU_PROPERTY_TYPE_0;         // Type
  memcpy(buf + 3, "GNU", 4);               // Name
  buf[4] = GNU_PROPERTY_X86_FEATURE_1_AND; // Feature type
  buf[5] = 4;                              // Feature size
  buf[6] = features;                       // Feature flags
}

template <typename E>
GabiCompressedSection<E>::GabiCompressedSection(Context<E> &ctx,
                                                Chunk<E> &chunk)
  : Chunk<E>(this->SYNTHETIC) {
  assert(chunk.name.starts_with(".debug"));
  this->name = chunk.name;

  std::unique_ptr<u8[]> buf(new u8[chunk.shdr.sh_size]);
  chunk.write_to(ctx, buf.get());

  chdr.ch_type = ELFCOMPRESS_ZLIB;
  chdr.ch_size = chunk.shdr.sh_size;
  chdr.ch_addralign = chunk.shdr.sh_addralign;

  contents.reset(new ZlibCompressor({(char *)buf.get(), chunk.shdr.sh_size}));

  this->shdr = chunk.shdr;
  this->shdr.sh_flags |= SHF_COMPRESSED;
  this->shdr.sh_addralign = 1;
  this->shdr.sh_size = sizeof(chdr) + contents->size();
  this->shndx = chunk.shndx;
}

template <typename E>
void GabiCompressedSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memcpy(base, &chdr, sizeof(chdr));
  contents->write_to(base + sizeof(chdr));
}

template <typename E>
GnuCompressedSection<E>::GnuCompressedSection(Context<E> &ctx,
                                              Chunk<E> &chunk)
  : Chunk<E>(this->SYNTHETIC) {
  assert(chunk.name.starts_with(".debug"));
  this->name = save_string(ctx, ".zdebug" + std::string(chunk.name.substr(6)));

  std::unique_ptr<u8[]> buf(new u8[chunk.shdr.sh_size]);
  chunk.write_to(ctx, buf.get());

  contents.reset(new ZlibCompressor({(char *)buf.get(), chunk.shdr.sh_size}));

  this->shdr = chunk.shdr;
  this->shdr.sh_size = HEADER_SIZE + contents->size();
  this->shndx = chunk.shndx;
  this->original_size = chunk.shdr.sh_size;
}

template <typename E>
void GnuCompressedSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memcpy(base, "ZLIB", 4);
  *(ubig64 *)(base + 4) = this->original_size;
  contents->write_to(base + 12);
}

template <typename E>
void ReproSection<E>::update_shdr(Context<E> &ctx) {
  if (contents)
    return;
  TarFile tar("repro");

  tar.append("response.txt", save_string(ctx, create_response_file(ctx)));
  tar.append("version.txt", save_string(ctx, mold_version + "\n"));

  std::unordered_set<std::string> seen;
  for (std::unique_ptr<MappedFile<Context<E>>> &mf : ctx.mf_pool) {
    std::string path = to_abs_path(mf->name);
    if (seen.insert(path).second)
      tar.append(path, mf->get_contents());
  }

  std::vector<u8> buf(tar.size());
  tar.write_to(&buf[0]);
  contents.reset(new GzipCompressor({(char *)&buf[0], buf.size()}));
  this->shdr.sh_size = contents->size();
}

template <typename E>
void ReproSection<E>::copy_buf(Context<E> &ctx) {
  contents->write_to(ctx.buf + this->shdr.sh_offset);
}

#define INSTANTIATE(E)                                                  \
  template class Chunk<E>;                                              \
  template class OutputEhdr<E>;                                         \
  template class OutputShdr<E>;                                         \
  template class OutputPhdr<E>;                                         \
  template class InterpSection<E>;                                      \
  template class OutputSection<E>;                                      \
  template class GotSection<E>;                                         \
  template class GotPltSection<E>;                                      \
  template class PltSection<E>;                                         \
  template class PltGotSection<E>;                                      \
  template class RelPltSection<E>;                                      \
  template class RelDynSection<E>;                                      \
  template class StrtabSection<E>;                                      \
  template class ShstrtabSection<E>;                                    \
  template class DynstrSection<E>;                                      \
  template class DynamicSection<E>;                                     \
  template class SymtabSection<E>;                                      \
  template class DynsymSection<E>;                                      \
  template class HashSection<E>;                                        \
  template class GnuHashSection<E>;                                     \
  template class MergedSection<E>;                                      \
  template class EhFrameSection<E>;                                     \
  template class EhFrameHdrSection<E>;                                  \
  template class DynbssSection<E>;                                      \
  template class VersymSection<E>;                                      \
  template class VerneedSection<E>;                                     \
  template class VerdefSection<E>;                                      \
  template class BuildIdSection<E>;                                     \
  template class NotePropertySection<E>;                                \
  template class GabiCompressedSection<E>;                              \
  template class GnuCompressedSection<E>;                               \
  template class ReproSection<E>;                                       \
  template i64 BuildId::size(Context<E> &) const;                       \
  template bool is_relro(Context<E> &, Chunk<E> *);                     \
  template bool separate_page(Context<E> &, Chunk<E> *, Chunk<E> *);    \
  template std::vector<ElfPhdr<E>> create_phdr(Context<E> &)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(ARM64);

} // namespace mold::elf
