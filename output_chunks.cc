#include "mold.h"

#include <openssl/rand.h>
#include <openssl/sha.h>
#include <shared_mutex>
#include <sys/mman.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>
#include <zlib.h>

template <typename E>
void OutputChunk<E>::write_to(Context<E> &ctx, u8 *buf) {
  Fatal(ctx) << name << ": write_to is called on an invalid section";
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
  if (!ctx.arg.entry.empty())
    hdr.e_entry = Symbol<E>::intern(ctx, ctx.arg.entry)->get_addr(ctx);
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
  for (OutputChunk<E> *chunk : ctx.chunks)
    if (chunk->shndx)
      n = chunk->shndx;
  this->shdr.sh_size = (n + 1) * sizeof(ElfShdr<E>);
}

template <typename E>
void OutputShdr<E>::copy_buf(Context<E> &ctx) {
  ElfShdr<E> *hdr = (ElfShdr<E> *)(ctx.buf + this->shdr.sh_offset);
  hdr[0] = {};

  for (OutputChunk<E> *chunk : ctx.chunks)
    if (chunk->shndx)
      hdr[chunk->shndx] = chunk->shdr;
}

template <typename E>
static i64 to_phdr_flags(OutputChunk<E> *chunk) {
  i64 ret = PF_R;
  if (chunk->shdr.sh_flags & SHF_WRITE)
    ret |= PF_W;
  if (chunk->shdr.sh_flags & SHF_EXECINSTR)
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
bool is_relro(Context<E> &ctx, OutputChunk<E> *chunk) {
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
    phdr.p_memsz = chunk->shdr.sh_size;
  };

  auto append = [&](OutputChunk<E> *chunk) {
    ElfPhdr<E> &phdr = vec.back();
    phdr.p_align = std::max<u64>(phdr.p_align, chunk->shdr.sh_addralign);
    phdr.p_filesz = (chunk->shdr.sh_type == SHT_NOBITS)
      ? chunk->shdr.sh_offset - phdr.p_offset
      : chunk->shdr.sh_offset + chunk->shdr.sh_size - phdr.p_offset;
    phdr.p_memsz = chunk->shdr.sh_addr + chunk->shdr.sh_size - phdr.p_vaddr;
  };

  auto is_bss = [](OutputChunk<E> *chunk) {
    return chunk->shdr.sh_type == SHT_NOBITS &&
           !(chunk->shdr.sh_flags & SHF_TLS);
  };

  // Create a PT_PHDR for the program header itself.
  define(PT_PHDR, PF_R, E::wordsize, ctx.phdr);

  // Create a PT_INTERP.
  if (ctx.interp)
    define(PT_INTERP, PF_R, 1, ctx.interp);

  // Create a PT_NOTE for each group of SHF_NOTE sections with the same
  // alignment requirement.
  for (i64 i = 0, end = ctx.chunks.size(); i < end;) {
    OutputChunk<E> *first = ctx.chunks[i++];
    if (first->shdr.sh_type != SHT_NOTE)
      continue;

    i64 flags = to_phdr_flags(first);
    i64 alignment = first->shdr.sh_addralign;
    define(PT_NOTE, flags, alignment, first);

    while (i < end && ctx.chunks[i]->shdr.sh_type == SHT_NOTE &&
           to_phdr_flags(ctx.chunks[i]) == flags &&
           ctx.chunks[i]->shdr.sh_addralign == alignment)
      append(ctx.chunks[i++]);
  }

  // Create PT_LOAD segments.
  for (i64 i = 0, end = ctx.chunks.size(); i < end;) {
    OutputChunk<E> *first = ctx.chunks[i++];
    if (!(first->shdr.sh_flags & SHF_ALLOC))
      break;

    i64 flags = to_phdr_flags(first);
    define(PT_LOAD, flags, PAGE_SIZE, first);
    first->new_page = true;

    if (!is_bss(first))
      while (i < end && !is_bss(ctx.chunks[i]) &&
             to_phdr_flags(ctx.chunks[i]) == flags)
        append(ctx.chunks[i++]);

    while (i < end && is_bss(ctx.chunks[i]) &&
           to_phdr_flags(ctx.chunks[i]) == flags)
      append(ctx.chunks[i++]);
  }

  // Create a PT_TLS.
  for (i64 i = 0; i < ctx.chunks.size(); i++) {
    if (!(ctx.chunks[i]->shdr.sh_flags & SHF_TLS))
      continue;

    define(PT_TLS, to_phdr_flags(ctx.chunks[i]), 1, ctx.chunks[i]);
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
      ctx.chunks[i]->new_page = true;
      i++;
      while (i < ctx.chunks.size() && is_relro(ctx, ctx.chunks[i]))
        append(ctx.chunks[i++]);
      ctx.chunks[i - 1]->new_page_end = true;
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
  write_vector(ctx.buf + this->shdr.sh_offset, create_phdr(ctx));
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
  for (ObjectFile<E> *file : ctx.objs) {
    file->reldyn_offset = offset;
    offset += file->num_dynrel * sizeof(ElfRel<E>);
  }
  this->shdr.sh_size = offset;
}

template <typename E>
void RelDynSection<E>::sort(Context<E> &ctx) {
  Timer t(ctx, "sort_dynamic_relocs");

  ElfRel<E> *begin = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset);
  ElfRel<E> *end = (ElfRel<E> *)((u8 *)begin + this->shdr.sh_size);

  tbb::parallel_sort(begin, end, [](const ElfRel<E> &a, const ElfRel<E> &b) {
    return std::tuple(a.r_type != E::R_IRELATIVE, a.r_sym, a.r_offset) <
           std::tuple(b.r_type != E::R_IRELATIVE, b.r_sym, b.r_offset);
  });
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

  for (OutputChunk<E> *chunk : ctx.chunks)
    if (!chunk->name.empty() && map.insert({chunk->name, offset}).second)
      offset += chunk->name.size() + 1;

  this->shdr.sh_size = offset;

  for (OutputChunk<E> *chunk : ctx.chunks)
    if (!chunk->name.empty())
      chunk->shdr.sh_name = map[chunk->name];
}

template <typename E>
void ShstrtabSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  base[0] = '\0';

  for (OutputChunk<E> *chunk : ctx.chunks)
    if (!chunk->name.empty())
      write_string(base + chunk->shdr.sh_name, chunk->name);
}

template <typename E>
i64 DynstrSection<E>::add_string(std::string_view str) {
  if (strings.empty())
    this->shdr.sh_size = 1;

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
void DynstrSection<E>::update_shdr(Context<E> &ctx) {
  if (this->shdr.sh_size == 1)
    this->shdr.sh_size = 0;
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
  for (OutputChunk<E> *chunk : ctx.chunks)
    if (chunk->shdr.sh_type == SHT_INIT_ARRAY)
      return true;
  return false;
}

template <typename E>
static bool has_fini_array(Context<E> &ctx) {
  for (OutputChunk<E> *chunk : ctx.chunks)
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

  if (Symbol<E> *sym = Symbol<E>::intern(ctx, ctx.arg.init); sym->file)
    define(DT_INIT, sym->get_addr(ctx));
  if (Symbol<E> *sym = Symbol<E>::intern(ctx, ctx.arg.fini); sym->file)
    define(DT_FINI, sym->get_addr(ctx));

  if (ctx.hash)
    define(DT_HASH, ctx.hash->shdr.sh_addr);
  if (ctx.gnu_hash)
    define(DT_GNU_HASH, ctx.gnu_hash->shdr.sh_addr);

  i64 flags = 0;
  i64 flags1 = 0;

  if (ctx.arg.pie)
    flags1 |= DF_1_PIE;

  if (ctx.arg.z_now) {
    flags |= DF_BIND_NOW;
    flags1 |= DF_1_NOW;
  }

  if (!ctx.arg.z_dlopen)
    flags1 |= DF_1_NOOPEN;
  if (!ctx.arg.z_delete)
    flags1 |= DF_1_NODELETE;
  if (ctx.arg.z_initfirst)
    flags1 |= DF_1_INITFIRST;
  if (ctx.arg.z_interpose)
    flags1 |= DF_1_INTERPOSE;

  if (ctx.has_gottp_rel)
    flags |= DF_STATIC_TLS;

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
  if (!ctx.arg.shared && ctx.dsos.empty())
    return;

  this->shdr.sh_size = create_dynamic_section(ctx).size() * E::wordsize;
  this->shdr.sh_link = ctx.dynstr->shndx;
}

template <typename E>
void DynamicSection<E>::copy_buf(Context<E> &ctx) {
  std::vector<typename E::WordTy> contents = create_dynamic_section(ctx);
  assert(this->shdr.sh_size == contents.size() * sizeof(contents[0]));
  write_vector(ctx.buf + this->shdr.sh_offset, contents);
}

static std::string_view get_output_name(std::string_view name) {
  static std::string_view prefixes[] = {
    ".text.hot.", ".text.unknown.", ".text.unlikely.", ".text.startup.",
    ".text.exit.", ".text.", ".data.rel.ro.", ".data.", ".rodata.",
    ".bss.rel.ro.", ".bss.", ".init_array.", ".fini_array.", ".tbss.",
    ".tdata.", ".gcc_except_table.",
  };

  for (std::string_view prefix : prefixes)
    if (name.starts_with(prefix))
      return prefix.substr(0, prefix.size() - 1);

  if (name == ".ctors" || name.starts_with(".ctors."))
    return ".init_array";
  if (name == ".dtors" || name.starts_with(".dtors."))
    return ".fini_array";

  if (name == ".zdebug_info")
    return ".debug_info";
  if (name == ".zdebug_aranges")
    return ".debug_aranges";
  if (name == ".zdebug_str")
    return ".debug_str";

  return name;
}

template <typename E>
OutputSection<E>::OutputSection(std::string_view name, u32 type,
                                u64 flags, u32 idx)
  : OutputChunk<E>(OutputChunk<E>::REGULAR), idx(idx) {
  this->name = name;
  this->shdr.sh_type = type;
  this->shdr.sh_flags = flags;
}

static u64 canonicalize_type(std::string_view name, u64 type) {
  if (type == SHT_PROGBITS && name == ".init_array")
    return SHT_INIT_ARRAY;
  if (type == SHT_PROGBITS && name == ".fini_array")
    return SHT_FINI_ARRAY;
  if (type == SHT_X86_64_UNWIND)
    return SHT_PROGBITS;
  return type;
}

template <typename E>
OutputSection<E> *
OutputSection<E>::get_instance(Context<E> &ctx, std::string_view name,
                               u64 type, u64 flags) {
  name = get_output_name(name);
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
  sym->set_got_idx(ctx, this->shdr.sh_size / E::got_size);
  this->shdr.sh_size += E::got_size;
  got_syms.push_back(sym);

  if (sym->is_imported)
    ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_gottp_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_gottp_idx(ctx, this->shdr.sh_size / E::got_size);
  this->shdr.sh_size += E::got_size;
  gottp_syms.push_back(sym);

  if (sym->is_imported)
    ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsgd_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_tlsgd_idx(ctx, this->shdr.sh_size / E::got_size);
  this->shdr.sh_size += E::got_size * 2;
  tlsgd_syms.push_back(sym);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsdesc_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_tlsdesc_idx(ctx, this->shdr.sh_size / E::got_size);
  this->shdr.sh_size += E::got_size * 2;
  tlsdesc_syms.push_back(sym);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsld(Context<E> &ctx) {
  if (tlsld_idx != -1)
    return;
  tlsld_idx = this->shdr.sh_size / E::got_size;
  this->shdr.sh_size += E::got_size * 2;
}

template <typename E>
u64 GotSection<E>::get_tlsld_addr(Context<E> &ctx) const {
  assert(tlsld_idx != -1);
  return this->shdr.sh_addr + tlsld_idx * E::got_size;
}

template <typename E>
i64 GotSection<E>::get_reldyn_size(Context<E> &ctx) const {
  i64 n = 0;
  for (Symbol<E> *sym : got_syms)
    if (sym->is_imported || (ctx.arg.pic && sym->is_relative(ctx)) ||
        sym->get_type() == STT_GNU_IFUNC)
      n++;

  n += tlsgd_syms.size() * 2;
  n += tlsdesc_syms.size();

  for (Symbol<E> *sym : gottp_syms)
    if (sym->is_imported)
      n++;

  if (tlsld_idx != -1)
    n++;

  n += ctx.dynbss->symbols.size();
  n += ctx.dynbss_relro->symbols.size();

  return n * sizeof(ElfRel<E>);
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
      if (ctx.arg.pic && sym->is_relative(ctx))
        *rel++ = reloc<E>(addr, E::R_RELATIVE, 0, (i64)sym->get_addr(ctx));
    }
  }

  for (Symbol<E> *sym : tlsgd_syms) {
    u64 addr = sym->get_tlsgd_addr(ctx);
    u32 dynsym_idx = sym->get_dynsym_idx(ctx);
    *rel++ = reloc<E>(addr, E::R_DTPMOD, dynsym_idx);
    *rel++ = reloc<E>(addr + E::got_size, E::R_DTPOFF, dynsym_idx);
  }

  for (Symbol<E> *sym : tlsdesc_syms)
    *rel++ = reloc<E>(sym->get_tlsdesc_addr(ctx), E::R_TLSDESC,
                      sym->get_dynsym_idx(ctx));

  for (Symbol<E> *sym : gottp_syms) {
    if (sym->is_imported)
      *rel++ = reloc<E>(sym->get_gottp_addr(ctx), E::R_TPOFF,
                        sym->get_dynsym_idx(ctx));
    else
      buf[sym->get_gottp_idx(ctx)] = sym->get_addr(ctx) - ctx.tls_end;
  }

  if (tlsld_idx != -1)
    *rel++ = reloc<E>(get_tlsld_addr(ctx), E::R_DTPMOD, 0);

  for (Symbol<E> *sym : ctx.dynbss->symbols)
    *rel++ = reloc<E>(sym->get_addr(ctx), E::R_COPY, sym->get_dynsym_idx(ctx));

  for (Symbol<E> *sym : ctx.dynbss_relro->symbols)
    *rel++ = reloc<E>(sym->get_addr(ctx), E::R_COPY, sym->get_dynsym_idx(ctx));
}

template <typename E>
void GotPltSection<E>::copy_buf(Context<E> &ctx) {
  typename E::WordTy *buf =
    (typename E::WordTy *)(ctx.buf + this->shdr.sh_offset);

  // The first slot of .got.plt points to _DYNAMIC, as requested by
  // the x86-64 psABI. The second and the third slots are reserved by
  // the psABI.
  buf[0] = ctx.dynamic ? ctx.dynamic->shdr.sh_addr : 0;
  buf[1] = 0;
  buf[2] = 0;

  for (Symbol<E> *sym : ctx.plt->symbols)
    buf[sym->get_gotplt_idx(ctx)] = sym->get_plt_addr(ctx) + 6;
}

template <typename E>
void PltSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(!sym->has_plt(ctx));
  assert(!sym->has_got(ctx));

  if (this->shdr.sh_size == 0) {
    this->shdr.sh_size = E::plt_size;
    ctx.gotplt->shdr.sh_size = E::got_size * 3;
  }

  sym->set_plt_idx(ctx, this->shdr.sh_size / E::plt_size);
  this->shdr.sh_size += E::plt_size;
  symbols.push_back(sym);

  sym->set_gotplt_idx(ctx, ctx.gotplt->shdr.sh_size / E::got_size);
  ctx.gotplt->shdr.sh_size += E::got_size;
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
  if (symbols.empty())
    symbols.push_back({});

  if (sym->get_dynsym_idx(ctx) != -1)
    return;
  sym->set_dynsym_idx(ctx, -2);
  symbols.push_back(sym);
}

template <typename E>
void DynsymSection<E>::sort_symbols(Context<E> &ctx) {
  Timer t(ctx, "sort_dynsyms");

  struct T {
    Symbol<E> *sym;
    i32 idx;
    u32 hash;

    bool is_local() const {
      return sym->esym().st_bind == STB_LOCAL;
    }
  };

  std::vector<T> vec(symbols.size());

  for (i32 i = 1; i < symbols.size(); i++)
    vec[i] = {symbols[i], i, 0};

  // In any ELF file, local symbols should precede global symbols.
  tbb::parallel_sort(vec.begin(), vec.end(), [](const T &a, const T &b) {
    return std::tuple(a.is_local(), a.idx) < std::tuple(b.is_local(), b.idx);
  });

  auto first_global = std::partition_point(vec.begin(), vec.end(),
                                           [](const T &x) {
    return x.is_local();
  });

  // In any ELF file, the index of the first global symbols can be
  // found in the symtab's sh_info field.
  this->shdr.sh_info = first_global - vec.begin();

  // If we have .gnu.hash section, it imposes more constraints
  // on the order of symbols.
  if (ctx.gnu_hash) {
    i64 num_globals = vec.end() - first_global;
    ctx.gnu_hash->num_buckets = num_globals / ctx.gnu_hash->LOAD_FACTOR + 1;
    ctx.gnu_hash->symoffset = first_global - vec.begin();

    tbb::parallel_for_each(first_global, vec.end(), [&](T &x) {
      x.hash = djb_hash(x.sym->name()) % ctx.gnu_hash->num_buckets;
    });

    tbb::parallel_sort(first_global, vec.end(), [&](const T &a, const T &b) {
      return std::tuple(a.hash, a.idx) < std::tuple(b.hash, b.idx);
    });
  }

  ctx.dynstr->dynsym_offset = ctx.dynstr->shdr.sh_size;

  for (i64 i = 1; i < symbols.size(); i++) {
    symbols[i] = vec[i].sym;
    symbols[i]->set_dynsym_idx(ctx, i);
    ctx.dynstr->shdr.sh_size += symbols[i]->name().size() + 1;
  }
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
    esym.st_type = sym.get_type();
    esym.st_size = sym.esym().st_size;

    if (sym.is_weak)
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
      if (!ctx.arg.pic && sym.has_plt(ctx) && !sym.has_got(ctx)) {
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
      esym.st_value = sym.get_addr(ctx);
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
void GnuHashSection<E>::update_shdr(Context<E> &ctx) {
  if (ctx.dynsym->symbols.empty())
    return;

  this->shdr.sh_link = ctx.dynsym->shndx;

  if (i64 num_symbols = ctx.dynsym->symbols.size() - symoffset) {
    // We allocate 12 bits for each symbol in the bloom filter.
    i64 num_bits = num_symbols * 12;
    num_bloom = next_power_of_two(num_bits / ELFCLASS_BITS);
  }

  i64 num_symbols = ctx.dynsym->symbols.size() - symoffset;

  this->shdr.sh_size = HEADER_SIZE;              // Header
  this->shdr.sh_size += num_bloom * E::wordsize; // Bloom filter
  this->shdr.sh_size += num_buckets * 4;         // Hash buckets
  this->shdr.sh_size += num_symbols * 4;         // Hash values
}

template <typename E>
void GnuHashSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memset(base, 0, this->shdr.sh_size);

  *(u32 *)base = num_buckets;
  *(u32 *)(base + 4) = symoffset;
  *(u32 *)(base + 8) = num_bloom;
  *(u32 *)(base + 12) = BLOOM_SHIFT;

  std::span<Symbol<E> *> symbols =
    std::span<Symbol<E> *>(ctx.dynsym->symbols).subspan(symoffset);

  std::vector<u32> hashes(symbols.size());
  for (i64 i = 0; i < symbols.size(); i++)
    hashes[i] = djb_hash(symbols[i]->name());

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
      buckets[idx] = i + symoffset;
  }

  // Write a hash table
  u32 *table = buckets + num_buckets;
  for (i64 i = 0; i < symbols.size(); i++) {
    bool is_last = false;
    if (i == symbols.size() - 1 ||
        (hashes[i] % num_buckets) != (hashes[i + 1] % num_buckets))
      is_last = true;

    if (is_last)
      table[i] = hashes[i] | 1;
    else
      table[i] = hashes[i] & ~1;
  }
}

template <typename E>
MergedSection<E> *
MergedSection<E>::get_instance(Context<E> &ctx, std::string_view name,
                               u64 type, u64 flags) {
  name = get_output_name(name);
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
MergedSection<E>::insert(std::string_view data, i64 alignment) {
  assert(alignment < UINT16_MAX);

  std::string_view suffix = data;
  if (suffix.size() > 32)
    suffix = suffix.substr(suffix.size() - 32);
  i64 shard = hash_string(suffix) % NUM_SHARDS;

  typename MapTy::const_accessor acc;
  bool inserted =
    maps[shard].insert(acc, std::pair(data, SectionFragment(this, data)));
  SectionFragment<E> *frag = const_cast<SectionFragment<E> *>(&acc->second);

  for (u16 cur = frag->alignment; cur < alignment;)
    if (frag->alignment.compare_exchange_strong(cur, alignment))
      break;

  for (u16 cur = max_alignment; cur < alignment;)
    if (max_alignment.compare_exchange_strong(cur, alignment))
      break;

  return frag;
}

template <typename E>
void MergedSection<E>::assign_offsets() {
  std::vector<SectionFragment<E> *> fragments[NUM_SHARDS];
  i64 sizes[NUM_SHARDS] = {};

  tbb::parallel_for((i64)0, NUM_SHARDS, [&](i64 i) {
    for (auto it = maps[i].begin(); it != maps[i].end(); it++)
      if (SectionFragment<E> &frag = it->second; frag.is_alive)
        fragments[i].push_back(&frag);

    // Sort section fragments to make an output deterministic.
    std::sort(fragments[i].begin(), fragments[i].end(),
              [&](SectionFragment<E> *a, SectionFragment<E> *b) {
                if (a->alignment != b->alignment)
                  return a->alignment > b->alignment;
                if (a->data.size() != b->data.size())
                  return a->data.size() < b->data.size();
                return a->data < b->data;
              });

    i64 offset = 0;
    for (SectionFragment<E> *frag : fragments[i]) {
      offset = align_to(offset, frag->alignment);
      frag->offset = offset;
      offset += frag->data.size();
    }

    sizes[i] = offset;
  });

  for (i64 i = 1; i < NUM_SHARDS + 1; i++)
    shard_offsets[i] =
      align_to(shard_offsets[i - 1] + sizes[i - 1], max_alignment);

  tbb::parallel_for((i64)1, NUM_SHARDS, [&](i64 i) {
    for (SectionFragment<E> *frag : fragments[i])
      frag->offset += shard_offsets[i];
  });

  this->shdr.sh_size = shard_offsets[NUM_SHARDS];
  this->shdr.sh_addralign = max_alignment;

  static Counter merged_strings("merged_strings");
  for (std::span<SectionFragment<E> *> span : fragments)
    merged_strings += span.size();
}

template <typename E>
void MergedSection<E>::copy_buf(Context<E> &ctx) {
  write_to(ctx, ctx.buf + this->shdr.sh_offset);
}

template <typename E>
void MergedSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  tbb::parallel_for((i64)0, NUM_SHARDS, [&](i64 i) {
    memset(buf + shard_offsets[i], 0, shard_offsets[i + 1] - shard_offsets[i]);
    for (auto it = maps[i].begin(); it != maps[i].end(); it++)
      if (SectionFragment<E> &frag = it->second; frag.is_alive)
        memcpy(buf + frag.offset, frag.data.data(), frag.data.size());
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
    Entry *entry = (Entry *)(base + HEADER_SIZE) + file->fde_idx;

    for (FdeRecord<E> &fde : file->fdes) {
      ElfRel<E> &rel = fde.cie->rels[fde.rel_idx];
      u64 val = file->symbols[rel.r_sym]->get_addr(ctx);
      u64 addend = fde.cie->input_section.get_addend(rel);
      i64 offset = file->fde_offset + fde.output_offset;

      *entry++ = {(i32)(val + addend - this->shdr.sh_addr),
                  (i32)(eh_frame_addr + offset - this->shdr.sh_addr)};
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
    unreachable(ctx);
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

  if (ctx.output_file->is_mmapped)
    munmap(buf, std::min(bufsize, shard_size));
}

template <typename E>
static std::vector<u8> get_uuid_v4(Context<E> &ctx) {
  std::vector<u8> buf(16);
  if (!RAND_bytes(buf.data(), buf.size()))
    Fatal(ctx) << "RAND_bytes failed";

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
    unreachable(ctx);
  }
}

template <typename E>
void NotePropertySection<E>::update_shdr(Context<E> &ctx) {
  features = -1;
  for (ObjectFile<E> *file : ctx.objs)
    if (file != ctx.internal_obj)
      features &= file->features;

  if (features != 0 && features != -1)
    this->shdr.sh_size = E::is_64 ? 32 : 28;
}

template <typename E>
void NotePropertySection<E>::copy_buf(Context<E> &ctx) {
  u32 *buf = (u32 *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  buf[0] = 4;                              // Name size
  buf[1] = E::is_64 ? 16 : 12;             // Content size
  buf[2] = NT_GNU_PROPERTY_TYPE_0;         // Type
  memcpy(buf + 3, "GNU", 4);               // Name
  buf[4] = GNU_PROPERTY_X86_FEATURE_1_AND; // Feature type
  buf[5] = 4;                              // Feature size
  buf[6] = features;                       // Feature flags
}

template <typename E>
CompressedSection<E>::CompressedSection(Context<E> &ctx, OutputChunk<E> &chunk)
  : OutputChunk<E>(this->SYNTHETIC) {
  assert(chunk.name.starts_with(".debug"));
  this->name = chunk.name;

  std::unique_ptr<u8[]> buf(new u8[chunk.shdr.sh_size]);
  chunk.write_to(ctx, buf.get());

  chdr.ch_type = ELFCOMPRESS_ZLIB;
  chdr.ch_size = chunk.shdr.sh_size;
  chdr.ch_addralign = chunk.shdr.sh_addralign;

  contents.reset(new Compress({(char *)buf.get(), chunk.shdr.sh_size}));

  this->shdr = chunk.shdr;
  this->shdr.sh_flags |= SHF_COMPRESSED;
  this->shdr.sh_addralign = 1;
  this->shdr.sh_size = sizeof(chdr) + contents->size();
  this->shndx = chunk.shndx;
}

template <typename E>
void CompressedSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  memcpy(base, &chdr, sizeof(chdr));
  contents->write_to(base + sizeof(chdr));
}

template <typename E>
void ReproSection<E>::update_shdr(Context<E> &ctx) {
  if (tar)
    return;
  tar = std::make_unique<TarFile>("repro");

  tar->append("response.txt", save_string(ctx, create_response_file(ctx)));
  tar->append("version.txt", save_string(ctx, get_version_string() + "\n"));

  std::unordered_set<std::string> seen;
  for (std::unique_ptr<MemoryMappedFile<E>> &mb : ctx.owning_mbs) {
    std::string path = path_to_absolute(mb->name);
    if (seen.insert(path).second)
      tar->append(path, mb->get_contents(ctx));
  }

  this->shdr.sh_size = tar->size();
}

template <typename E>
void ReproSection<E>::copy_buf(Context<E> &ctx) {
  tar->write(ctx.buf + this->shdr.sh_offset);
}

#define INSTANTIATE(E)                                          \
  template class OutputChunk<E>;                                \
  template class OutputEhdr<E>;                                 \
  template class OutputShdr<E>;                                 \
  template class OutputPhdr<E>;                                 \
  template class InterpSection<E>;                              \
  template class OutputSection<E>;                              \
  template class GotSection<E>;                                 \
  template class GotPltSection<E>;                              \
  template class PltSection<E>;                                 \
  template class PltGotSection<E>;                              \
  template class RelPltSection<E>;                              \
  template class RelDynSection<E>;                              \
  template class StrtabSection<E>;                              \
  template class ShstrtabSection<E>;                            \
  template class DynstrSection<E>;                              \
  template class DynamicSection<E>;                             \
  template class SymtabSection<E>;                              \
  template class DynsymSection<E>;                              \
  template class HashSection<E>;                                \
  template class GnuHashSection<E>;                             \
  template class MergedSection<E>;                              \
  template class EhFrameSection<E>;                             \
  template class EhFrameHdrSection<E>;                          \
  template class DynbssSection<E>;                              \
  template class VersymSection<E>;                              \
  template class VerneedSection<E>;                             \
  template class VerdefSection<E>;                              \
  template class BuildIdSection<E>;                             \
  template class NotePropertySection<E>;                        \
  template class CompressedSection<E>;                          \
  template class ReproSection<E>;                               \
  template i64 BuildId::size(Context<E> &) const;               \
  template bool is_relro(Context<E> &, OutputChunk<E> *);       \
  template std::vector<ElfPhdr<E>> create_phdr(Context<E> &)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
