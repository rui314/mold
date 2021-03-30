#include "mold.h"

#include <openssl/rand.h>
#include <openssl/sha.h>
#include <shared_mutex>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>

template <typename E>
i64 BuildId::size(Context<E> &ctx) const {
  switch (kind) {
  case HEX:
    return value.size();
  case HASH:
    return hash_size;
  case UUID:
    return 16;
  }
  unreachable(ctx);
}

template <typename E>
void OutputEhdr<E>::copy_buf(Context<E> &ctx) {
  ElfEhdr<E> &hdr = *(ElfEhdr<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(&hdr, 0, sizeof(hdr));

  memcpy(&hdr.e_ident, "\177ELF", 4);
  hdr.e_ident[EI_CLASS] = ELFCLASS64;
  hdr.e_ident[EI_DATA] = ELFDATA2LSB;
  hdr.e_ident[EI_VERSION] = EV_CURRENT;
  hdr.e_type = ctx.arg.pic ? ET_DYN : ET_EXEC;
  hdr.e_machine = EM_X86_64;
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
  i64 n = 1;
  for (OutputChunk<E> *chunk : ctx.chunks)
    if (chunk->kind != OutputChunk<E>::HEADER)
      n++;
  this->shdr.sh_size = n * sizeof(ElfShdr<E>);
}

template <typename E>
void OutputShdr<E>::copy_buf(Context<E> &ctx) {
  ElfShdr<E> *hdr = (ElfShdr<E> *)(ctx.buf + this->shdr.sh_offset);
  hdr[0] = {};

  i64 i = 1;
  for (OutputChunk<E> *chunk : ctx.chunks)
    if (chunk->kind != OutputChunk<E>::HEADER)
      hdr[i++] = chunk->shdr;
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

template <typename E>
bool is_relro(Context<E> &ctx, OutputChunk<E> *chunk) {
  u64 flags = chunk->shdr.sh_flags;
  u64 type = chunk->shdr.sh_type;
  std::string_view name = chunk->name;

  bool match = (flags & SHF_TLS) || type == SHT_INIT_ARRAY ||
               type == SHT_FINI_ARRAY || type == SHT_PREINIT_ARRAY ||
               chunk == ctx.got || chunk == ctx.dynamic ||
               name.ends_with(".rel.ro");

  return (flags & SHF_WRITE) && match;
}

template <typename E>
std::vector<ElfPhdr<E>> create_phdr(Context<E> &ctx) {
  std::vector<ElfPhdr<E>> vec;

  auto define = [&](u64 type, u64 flags, i64 min_align, OutputChunk<E> *chunk) {
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
  define(PT_PHDR, PF_R, 8, ctx.phdr);

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
    offset += file->num_dynrel * sizeof(ElfRela<E>);
  }
  this->shdr.sh_size = offset;
}

template <typename E>
void RelDynSection<E>::sort(Context<E> &ctx) {
  Timer t("sort_dynamic_relocs");

  ElfRela<E> *begin = (ElfRela<E> *)(ctx.buf + this->shdr.sh_offset);
  ElfRela<E> *end =
    (ElfRela<E> *)(ctx.buf + this->shdr.sh_offset + this->shdr.sh_size);

  tbb::parallel_sort(begin, end, [](const ElfRela<E> &a, const ElfRela<E> &b) {
    return std::tuple(a.r_sym, a.r_offset) <
           std::tuple(b.r_sym, b.r_offset);
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
  this->shdr.sh_size = 1;
  for (OutputChunk<E> *chunk : ctx.chunks) {
    if (!chunk->name.empty()) {
      chunk->shdr.sh_name = this->shdr.sh_size;
      this->shdr.sh_size += chunk->name.size() + 1;
    }
  }
}

template <typename E>
void ShstrtabSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;
  base[0] = '\0';

  i64 i = 1;
  for (OutputChunk<E> *chunk : ctx.chunks) {
    if (!chunk->name.empty()) {
      write_string(base + i, chunk->name);
      i += chunk->name.size() + 1;
    }
  }
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
      write_string(base + offset, sym->name);
      offset += sym->name.size() + 1;
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
static std::vector<u64> create_dynamic_section(Context<E> &ctx) {
  std::vector<u64> vec;

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
    define(DT_RELA, ctx.reldyn->shdr.sh_addr);
    define(DT_RELASZ, ctx.reldyn->shdr.sh_size);
    define(DT_RELAENT, sizeof(ElfRela<E>));
  }

  if (ctx.relplt->shdr.sh_size) {
    define(DT_JMPREL, ctx.relplt->shdr.sh_addr);
    define(DT_PLTRELSZ, ctx.relplt->shdr.sh_size);
    define(DT_PLTREL, DT_RELA);
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

  if (ctx.has_gottpoff)
    flags |= DF_STATIC_TLS;

  if (flags)
    define(DT_FLAGS, flags);
  if (flags1)
    define(DT_FLAGS_1, flags1);

  define(DT_DEBUG, 0);
  define(DT_NULL, 0);
  return vec;
}

template <typename E>
void DynamicSection<E>::update_shdr(Context<E> &ctx) {
  if (ctx.arg.is_static)
    return;
  if (!ctx.arg.shared && ctx.dsos.empty())
    return;

  this->shdr.sh_size = create_dynamic_section(ctx).size() * 8;
  this->shdr.sh_link = ctx.dynstr->shndx;
}

template <typename E>
void DynamicSection<E>::copy_buf(Context<E> &ctx) {
  std::vector<u64> contents = create_dynamic_section(ctx);
  assert(this->shdr.sh_size == contents.size() * sizeof(contents[0]));
  write_vector(ctx.buf + this->shdr.sh_offset, contents);
}

static std::string_view get_output_name(std::string_view name) {
  static std::string_view prefixes[] = {
    ".text.", ".data.rel.ro.", ".data.", ".rodata.", ".bss.rel.ro.",
    ".bss.", ".init_array.", ".fini_array.", ".tbss.", ".tdata.",
  };

  for (std::string_view prefix : prefixes)
    if (name.starts_with(prefix))
      return prefix.substr(0, prefix.size() - 1);

  if (name == ".zdebug_info")
    return ".debug_info";
  if (name == ".zdebug_aranges")
    return ".debug_aranges";
  if (name == ".zdebug_str")
    return ".debug_str";

  return name;
}

template <typename E>
OutputSection<E>::OutputSection(std::string_view name, u32 type, u64 flags)
  : OutputChunk<E>(OutputChunk<E>::REGULAR) {
  this->name = name;
  this->shdr.sh_type = type;
  this->shdr.sh_flags = flags;
  idx = instances.size();
  instances.push_back(this);
}

template <typename E>
OutputSection<E> *
OutputSection<E>::get_instance(std::string_view name, u64 type, u64 flags) {
  if (name == ".eh_frame" && type == SHT_X86_64_UNWIND)
    type = SHT_PROGBITS;

  name = get_output_name(name);
  flags = flags & ~(u64)SHF_GROUP & ~(u64)SHF_COMPRESSED;

  auto find = [&]() -> OutputSection<E> * {
    for (OutputSection<E> *osec : OutputSection::instances)
      if (name == osec->name && type == osec->shdr.sh_type &&
          flags == osec->shdr.sh_flags)
        return osec;
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
  return new OutputSection(name, type, flags);
}

template <typename E>
void OutputSection<E>::copy_buf(Context<E> &ctx) {
  if (this->shdr.sh_type == SHT_NOBITS)
    return;

  tbb::parallel_for((i64)0, (i64)members.size(), [&](i64 i) {
    // Copy section contents to an output file
    InputSection<E> &isec = *members[i];
    isec.copy_buf(ctx);

    // Zero-clear trailing padding
    u64 this_end = isec.offset + isec.shdr.sh_size;
    u64 next_start = (i == members.size() - 1) ?
      this->shdr.sh_size : members[i + 1]->offset;
    memset(ctx.buf + this->shdr.sh_offset + this_end, 0, next_start - this_end);
  });
}

template <typename E>
void GotSection<E>::add_got_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(sym->got_idx == -1);
  sym->got_idx = this->shdr.sh_size / GOT_SIZE;
  this->shdr.sh_size += GOT_SIZE;
  got_syms.push_back(sym);

  if (sym->is_imported)
    ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_gottpoff_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(sym->gottpoff_idx == -1);
  sym->gottpoff_idx = this->shdr.sh_size / GOT_SIZE;
  this->shdr.sh_size += GOT_SIZE;
  gottpoff_syms.push_back(sym);

  if (sym->is_imported)
    ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsgd_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(sym->tlsgd_idx == -1);
  sym->tlsgd_idx = this->shdr.sh_size / GOT_SIZE;
  this->shdr.sh_size += GOT_SIZE * 2;
  tlsgd_syms.push_back(sym);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsdesc_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(sym->tlsdesc_idx == -1);
  sym->tlsdesc_idx = this->shdr.sh_size / GOT_SIZE;
  this->shdr.sh_size += GOT_SIZE * 2;
  tlsdesc_syms.push_back(sym);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void GotSection<E>::add_tlsld(Context<E> &ctx) {
  if (tlsld_idx != -1)
    return;
  tlsld_idx = this->shdr.sh_size / GOT_SIZE;
  this->shdr.sh_size += GOT_SIZE * 2;
}

template <typename E>
i64 GotSection<E>::get_reldyn_size(Context<E> &ctx) const {
  i64 n = 0;
  for (Symbol<E> *sym : got_syms)
    if (sym->is_imported || (ctx.arg.pic && sym->is_relative(ctx)))
      n++;

  n += tlsgd_syms.size() * 2;
  n += tlsdesc_syms.size() * 2;

  for (Symbol<E> *sym : gottpoff_syms)
    if (sym->is_imported)
      n++;

  if (tlsld_idx != -1)
    n++;

  n += ctx.dynbss->symbols.size();
  n += ctx.dynbss_relro->symbols.size();

  return n * sizeof(ElfRela<E>);
}

// Fill .got and .rel.dyn.
template <typename E>
void GotSection<E>::copy_buf(Context<E> &ctx) {
  u64 *buf = (u64 *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  ElfRela<E> *rel = (ElfRela<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset);

  for (Symbol<E> *sym : got_syms) {
    u64 addr = sym->get_got_addr(ctx);
    if (sym->is_imported) {
      *rel++ = {addr, R_X86_64_GLOB_DAT, sym->dynsym_idx, 0};
    } else {
      buf[sym->got_idx] = sym->get_addr(ctx);
      if (ctx.arg.pic && sym->is_relative(ctx))
        *rel++ = {addr, R_X86_64_RELATIVE, 0, (i64)sym->get_addr(ctx)};
    }
  }

  for (Symbol<E> *sym : tlsgd_syms) {
    u64 addr = sym->get_tlsgd_addr(ctx);
    *rel++ = {addr, R_X86_64_DTPMOD64, sym->dynsym_idx, 0};
    *rel++ = {addr + GOT_SIZE, R_X86_64_DTPOFF64, sym->dynsym_idx, 0};
  }

  for (Symbol<E> *sym : tlsdesc_syms)
    *rel++ = {sym->get_tlsdesc_addr(ctx), R_X86_64_TLSDESC, sym->dynsym_idx, 0};

  for (Symbol<E> *sym : gottpoff_syms) {
    if (sym->is_imported)
      *rel++ =
        {sym->get_gottpoff_addr(ctx), R_X86_64_TPOFF64, sym->dynsym_idx, 0};
    else
      buf[sym->gottpoff_idx] = sym->get_addr(ctx) - ctx.tls_end;
  }

  if (tlsld_idx != -1)
    *rel++ = {get_tlsld_addr(ctx), R_X86_64_DTPMOD64, 0, 0};

  for (Symbol<E> *sym : ctx.dynbss->symbols)
    *rel++ = {sym->get_addr(ctx), R_X86_64_COPY, sym->dynsym_idx, 0};

  for (Symbol<E> *sym : ctx.dynbss_relro->symbols)
    *rel++ = {sym->get_addr(ctx), R_X86_64_COPY, sym->dynsym_idx, 0};
}

template <typename E>
void GotPltSection<E>::copy_buf(Context<E> &ctx) {
  u64 *buf = (u64 *)(ctx.buf + this->shdr.sh_offset);

  // The first slot of .got.plt points to _DYNAMIC, as requested by
  // the x86-64 psABI. The second and the third slots are reserved by
  // the psABI.
  buf[0] = ctx.dynamic ? ctx.dynamic->shdr.sh_addr : 0;
  buf[1] = 0;
  buf[2] = 0;

  for (Symbol<E> *sym : ctx.plt->symbols)
    if (sym->gotplt_idx != -1)
      buf[sym->gotplt_idx] = sym->get_plt_addr(ctx) + 6;
}

template <typename E>
void PltSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(sym->plt_idx == -1);
  assert(sym->got_idx == -1);

  if (this->shdr.sh_size == 0) {
    this->shdr.sh_size = PLT_SIZE;
    ctx.gotplt->shdr.sh_size = GOT_SIZE * 3;
  }

  sym->plt_idx = this->shdr.sh_size / PLT_SIZE;
  this->shdr.sh_size += PLT_SIZE;
  symbols.push_back(sym);

  sym->gotplt_idx = ctx.gotplt->shdr.sh_size / GOT_SIZE;
  ctx.gotplt->shdr.sh_size += GOT_SIZE;
  ctx.relplt->shdr.sh_size += sizeof(ElfRela<E>);
  ctx.dynsym->add_symbol(ctx, sym);
}

template <typename E>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  static const u8 plt0[] = {
    0xff, 0x35, 0, 0, 0, 0, // pushq GOTPLT+8(%rip)
    0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
    0x0f, 0x1f, 0x40, 0x00, // nop
  };

  memcpy(buf, plt0, sizeof(plt0));
  *(u32 *)(buf + 2) = ctx.gotplt->shdr.sh_addr - this->shdr.sh_addr + 2;
  *(u32 *)(buf + 8) = ctx.gotplt->shdr.sh_addr - this->shdr.sh_addr + 4;

  i64 relplt_idx = 0;

  static const u8 data[] = {
    0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOTPLT
    0x68, 0,    0, 0, 0,    // push  $index_in_relplt
    0xe9, 0,    0, 0, 0,    // jmp   PLT[0]
  };

  for (Symbol<E> *sym : symbols) {
    u8 *ent = buf + sym->plt_idx * PLT_SIZE;
    memcpy(ent, data, sizeof(data));
    *(u32 *)(ent + 2) = sym->get_gotplt_addr(ctx) - sym->get_plt_addr(ctx) - 6;
    *(u32 *)(ent + 7) = relplt_idx++;
    *(u32 *)(ent + 12) = this->shdr.sh_addr - sym->get_plt_addr(ctx) - 16;
  }
}

template <typename E>
void PltGotSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  assert(sym->plt_idx == -1);
  assert(sym->got_idx != -1);

  sym->plt_idx = this->shdr.sh_size / PLT_GOT_SIZE;
  this->shdr.sh_size += PLT_GOT_SIZE;
  symbols.push_back(sym);
}

template <typename E>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  static const u8 data[] = {
    0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOT
    0x66, 0x90,             // nop
  };

  for (Symbol<E> *sym : symbols) {
    u8 *ent = buf + sym->plt_idx * PLT_GOT_SIZE;
    memcpy(ent, data, sizeof(data));
    *(u32 *)(ent + 2) = sym->get_got_addr(ctx) - sym->get_plt_addr(ctx) - 6;
  }
}

template <typename E>
void RelPltSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_link = ctx.dynsym->shndx;
}

template <typename E>
void RelPltSection<E>::copy_buf(Context<E> &ctx) {
  ElfRela<E> *buf = (ElfRela<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  i64 relplt_idx = 0;

  for (Symbol<E> *sym : ctx.plt->symbols) {
    ElfRela<E> &rel = buf[relplt_idx++];
    memset(&rel, 0, sizeof(rel));
    rel.r_sym = sym->dynsym_idx;
    rel.r_offset = sym->get_gotplt_addr(ctx);

    if (sym->get_type() == STT_GNU_IFUNC) {
      rel.r_type = R_X86_64_IRELATIVE;
      rel.r_addend = sym->input_section->get_addr() + sym->value;
    } else {
      rel.r_type = R_X86_64_JUMP_SLOT;
    }
  }
}

template <typename E>
void DynsymSection<E>::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  if (symbols.empty())
    symbols.push_back({});

  if (sym->dynsym_idx != -1)
    return;
  sym->dynsym_idx = -2;
  symbols.push_back(sym);
}

template <typename E>
void DynsymSection<E>::sort_symbols(Context<E> &ctx) {
  Timer t("sort_dynsyms");

  struct T {
    Symbol<E> *sym;
    i32 idx;
    u32 hash;

    bool is_local() const {
      return sym->esym->st_bind == STB_LOCAL;
    }
  };

  std::vector<T> vec(symbols.size());

  for (i32 i = 1; i < symbols.size(); i++)
    vec[i] = {symbols[i], i, 0};

  // In any ELF file, local symbols should precede global symbols.
  tbb::parallel_sort(vec.begin() + 1, vec.end(), [](const T &a, const T &b) {
    return std::tuple(a.is_local(), a.idx) < std::tuple(b.is_local(), b.idx);
  });

  auto first_global = std::partition_point(vec.begin() + 1, vec.end(),
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
      x.hash = gnu_hash(x.sym->name) % ctx.gnu_hash->num_buckets;
    });

    tbb::parallel_sort(first_global, vec.end(), [&](const T &a, const T &b) {
      return std::tuple(a.hash, a.idx) < std::tuple(b.hash, b.idx);
    });
  }

  ctx.dynstr->dynsym_offset = ctx.dynstr->shdr.sh_size;

  for (i64 i = 1; i < symbols.size(); i++) {
    symbols[i] = vec[i].sym;
    symbols[i]->dynsym_idx = i;
    ctx.dynstr->shdr.sh_size += symbols[i]->name.size() + 1;
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

    ElfSym<E> &esym = *(ElfSym<E> *)(base + sym.dynsym_idx * sizeof(ElfSym<E>));
    memset(&esym, 0, sizeof(esym));
    esym.st_type = sym.get_type();
    esym.st_size = sym.esym->st_size;

    if (sym.is_weak)
      esym.st_bind = STB_WEAK;
    else if (sym.file->is_dso)
      esym.st_bind = STB_GLOBAL;
    else
      esym.st_bind = sym.esym->st_bind;

    esym.st_name = name_offset;
    name_offset += sym.name.size() + 1;

    if (sym.has_copyrel) {
      esym.st_shndx = sym.copyrel_readonly
        ? ctx.dynbss_relro->shndx : ctx.dynbss->shndx;
      esym.st_value = sym.get_addr(ctx);
    } else if (sym.file->is_dso || sym.esym->is_undef()) {
      esym.st_shndx = SHN_UNDEF;
      esym.st_size = 0;
      if (!ctx.arg.shared && sym.plt_idx != -1 && sym.got_idx == -1) {
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
    i64 idx = elf_hash(sym->name) % num_slots;
    chains[sym->dynsym_idx] = buckets[idx];
    buckets[idx] = sym->dynsym_idx;
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

  this->shdr.sh_size = HEADER_SIZE;                    // Header
  this->shdr.sh_size += num_bloom * ELFCLASS_BITS / 8; // Bloom filter
  this->shdr.sh_size += num_buckets * 4;               // Hash buckets
  this->shdr.sh_size += num_symbols * 4;               // Hash values
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
    hashes[i] = gnu_hash(symbols[i]->name);

  // Write a bloom filter
  u64 *bloom = (u64 *)(base + HEADER_SIZE);
  for (i64 hash : hashes) {
    i64 idx = (hash / 64) % num_bloom;
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
MergedSection<E>::get_instance(std::string_view name, u64 type, u64 flags) {
  name = get_output_name(name);
  flags = flags & ~(u64)SHF_MERGE & ~(u64)SHF_STRINGS;

  auto find = [&]() -> MergedSection * {
    for (MergedSection *osec : MergedSection::instances)
      if (std::tuple(name, flags, type) ==
          std::tuple(osec->name, osec->shdr.sh_flags, osec->shdr.sh_type))
        return osec;
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
  MergedSection::instances.push_back(osec);
  return osec;
}

static void update_atomic_max(std::atomic_uint16_t &atom, i16 val) {
  u16 cur = atom;
  while (cur < val)
    if (atom.compare_exchange_strong(cur, val))
      break;
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

  update_atomic_max(frag->alignment, alignment);
  update_atomic_max(max_alignment, alignment);
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
  u8 *base = ctx.buf + this->shdr.sh_offset;

  tbb::parallel_for((i64)0, NUM_SHARDS, [&](i64 i) {
    memset(base + shard_offsets[i], 0, shard_offsets[i + 1] - shard_offsets[i]);

    for (auto it = maps[i].begin(); it != maps[i].end(); it++)
      if (SectionFragment<E> &frag = it->second; frag.is_alive)
        memcpy(base + frag.offset, frag.data.data(), frag.data.size());
  });
}

template <typename E>
void EhFrameSection<E>::construct(Context<E> &ctx) {
  // Remove dead FDEs and assign them offsets within their corresponding
  // CIE group.
  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    ObjectFile<E> *file = ctx.objs[i];
    i64 count = 0;

    for (CieRecord<E> &cie : file->cies) {
      i64 offset = 0;
      for (FdeRecord<E> &fde : cie.fdes) {
        if (!fde.is_alive)
          continue;
        fde.offset = offset;
        offset += fde.contents.size();
        cie.num_fdes++;
      }
      cie.fde_size = offset;
    }
  });

  // Aggreagate CIEs.
  cies.reserve(ctx.objs.size());
  for (ObjectFile<E> *file : ctx.objs)
    for (CieRecord<E> &cie : file->cies)
      cies.push_back(&cie);

  // Record the total number of FDEs for .eh_frame_hdr.
  for (CieRecord<E> *cie : cies) {
    cie->fde_idx = num_fdes;
    num_fdes += cie->num_fdes;
  }

  // Assign offsets within the output section to CIEs.
  auto should_merge = [](CieRecord<E> &a, CieRecord<E> &b) {
    return a.contents == b.contents && a.rels == b.rels;
  };

  i64 offset = 0;
  for (i64 i = 0; i < cies.size(); i++) {
    CieRecord<E> &cie = *cies[i];
    cie.offset = offset;

    if (i == 0 || !should_merge(cie, *cies[i - 1])) {
      cie.leader_offset = offset;
      offset += cie.contents.size() + cie.fde_size;
    } else {
      cie.leader_offset = cies[i - 1]->leader_offset;
      offset += cie.fde_size;
    }
  }
  this->shdr.sh_size = offset;

  if (ctx.eh_frame_hdr)
    ctx.eh_frame_hdr->shdr.sh_size =
      ctx.eh_frame_hdr->HEADER_SIZE + num_fdes * 8;
}

template <typename E>
void EhFrameSection<E>::copy_buf(Context<E> &ctx) {
  u8 *base = ctx.buf + this->shdr.sh_offset;

  u8 *hdr_base = nullptr;
  if (ctx.eh_frame_hdr)
    hdr_base = ctx.buf + ctx.eh_frame_hdr->shdr.sh_offset;

  auto apply_reloc = [&](EhReloc<E> &rel, u64 loc, u64 val) {
    switch (rel.type) {
    case R_X86_64_32:
      *(u32 *)(base + loc) = val;
      return;
    case R_X86_64_64:
      *(u64 *)(base + loc) = val;
      return;
    case R_X86_64_PC32:
      *(u32 *)(base + loc) = val - this->shdr.sh_addr - loc;
      return;
    case R_X86_64_PC64:
      *(u64 *)(base + loc) = val - this->shdr.sh_addr - loc;
      return;
    }
    unreachable(ctx);
  };

  struct Entry {
    i32 init_addr;
    i32 fde_addr;
  };

  // Copy CIEs and FDEs.
  tbb::parallel_for_each(cies, [&](CieRecord<E> *cie) {
    i64 cie_size = 0;

    Entry *entry = nullptr;
    if (ctx.eh_frame_hdr)
      entry = (Entry *)(hdr_base + ctx.eh_frame_hdr->HEADER_SIZE) +
              cie->fde_idx;

    // Copy a CIE.
    if (cie->offset == cie->leader_offset) {
      memcpy(base + cie->offset, cie->contents.data(), cie->contents.size());
      cie_size = cie->contents.size();

      for (EhReloc<E> &rel : cie->rels) {
        u64 loc = cie->offset + rel.offset;
        u64 val = rel.sym.get_addr(ctx) + rel.addend;
        apply_reloc(rel, loc, val);
      }
    }

    // Copy FDEs.
    for (FdeRecord<E> &fde : cie->fdes) {
      if (fde.offset == -1)
        continue;

      i64 fde_off = cie->offset + cie_size + fde.offset;
      memcpy(base + fde_off, fde.contents.data(), fde.contents.size());
      *(u32 *)(base + fde_off + 4) = fde_off + 4 - cie->leader_offset;

      for (i64 i = 0; i < fde.rels.size(); i++) {
        EhReloc<E> &rel = fde.rels[i];
        u64 loc = fde_off + rel.offset;
        u64 val = rel.sym.get_addr(ctx) + rel.addend;
        apply_reloc(rel, loc, val);

        // Write to .eh_frame_hdr
        if (ctx.eh_frame_hdr && i == 0) {
          assert(rel.offset == 8);
          entry->init_addr = val - ctx.eh_frame_hdr->shdr.sh_addr;
          entry->fde_addr =
            this->shdr.sh_addr + fde_off - ctx.eh_frame_hdr->shdr.sh_addr;
          entry++;
        }
      }
    }
  });

  if (ctx.eh_frame_hdr) {
    // Write .eh_frame_hdr header
    hdr_base[0] = 1;
    hdr_base[1] = DW_EH_PE_pcrel | DW_EH_PE_sdata4;
    hdr_base[2] = DW_EH_PE_udata4;
    hdr_base[3] = DW_EH_PE_datarel | DW_EH_PE_sdata4;

    *(u32 *)(hdr_base + 4) =
      this->shdr.sh_addr - ctx.eh_frame_hdr->shdr.sh_addr - 4;
    *(u32 *)(hdr_base + 8) = num_fdes;

    // Sort .eh_frame_hdr contents
    Entry *begin = (Entry *)(hdr_base + ctx.eh_frame_hdr->HEADER_SIZE);
    Entry *end = begin + num_fdes;

    tbb::parallel_sort(begin, end, [](const Entry &a, const Entry &b) {
      return a.init_addr < b.init_addr;
    });
  }
}

// Compiler-generated object files don't usually contain symbols
// referring a .eh_frame section, but crtend.o contains such symbol
// (i.e. "__FRAME_END__"). So we need to handle such symbol.
// This function is slow, but it's okay because they are rare.
template <typename E>
u64 EhFrameSection<E>::get_addr(Context<E> &ctx, const Symbol<E> &sym) {
  InputSection<E> &isec = *sym.input_section;
  const char *section_begin = isec.contents.data();

  auto contains = [](std::string_view str, const char *ptr) {
    const char *begin = str.data();
    const char *end = begin + str.size();
    return (begin == ptr) || (begin < ptr && ptr < end);
  };

  for (CieRecord<E> &cie : isec.file.cies) {
    u64 offset = 0;

    if (cie.offset == cie.leader_offset) {
      if (contains(cie.contents, section_begin + offset)) {
        u64 cie_addr = this->shdr.sh_addr + cie.offset;
        u64 addend = sym.value - offset;
        return cie_addr + addend;
      }
      offset += cie.contents.size();
    }

    for (FdeRecord<E> &fde : cie.fdes) {
      if (contains(fde.contents, section_begin + offset)) {
        if (!fde.is_alive)
          return 0;

        u64 fde_addr = this->shdr.sh_addr + cie.offset + offset;
        u64 addend = sym.value - offset;
        return fde_addr + addend;
      }
      offset += fde.contents.size();
    }
  }

  Fatal(ctx) << isec.file << ": .eh_frame has bad symbol: " << sym;
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
  this->shdr.sh_size += sym->esym->st_size;
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
void VerneedSection<E>::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = contents.size();
  this->shdr.sh_link = ctx.dynstr->shndx;
}

template <typename E>
void VerneedSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->shdr.sh_offset, contents);
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

static void compute_sha256(u8 *buf, i64 size, u8 *digest) {
  i64 shard_size = 1024 * 1024;
  i64 num_shards = size / shard_size + 1;
  std::vector<u8> shards(num_shards * SHA256_SIZE);

  tbb::parallel_for((i64)0, num_shards, [&](i64 i) {
    u8 *begin = buf + shard_size * i;
    i64 sz = (i < num_shards - 1) ? shard_size : (size % shard_size);
    SHA256(begin, sz, shards.data() + i * SHA256_SIZE);
  });

  SHA256(shards.data(), shards.size(), digest);
}

template <typename E>
void BuildIdSection<E>::write_buildid(Context<E> &ctx, i64 filesize) {
  switch (ctx.arg.build_id.kind) {
  case BuildId::HEX:
    write_vector(ctx.buf + this->shdr.sh_offset + HEADER_SIZE,
                 ctx.arg.build_id.value);
    return;
  case BuildId::HASH: {
    // Modern x86 processors have purpose-built instructions to accelerate
    // SHA256 computation, and SHA256 outperforms MD5 on such computers.
    // So, we always compute SHA256 and truncate it if smaller digest was
    // requested.
    u8 digest[SHA256_SIZE];
    assert(ctx.arg.build_id.size(ctx) <= SHA256_SIZE);
    compute_sha256(ctx.buf, filesize, digest);
    memcpy(ctx.buf + this->shdr.sh_offset + HEADER_SIZE, digest,
           ctx.arg.build_id.size(ctx));
    return;
  }
  case BuildId::UUID:
    if (!RAND_bytes(ctx.buf + this->shdr.sh_offset + HEADER_SIZE,
                    ctx.arg.build_id.size(ctx)))
      Fatal(ctx) << "RAND_bytes failed";
    return;
  }

  unreachable(ctx);
}

template class OutputChunk<X86_64>;
template class OutputEhdr<X86_64>;
template class OutputShdr<X86_64>;
template class OutputPhdr<X86_64>;
template class InterpSection<X86_64>;
template class OutputSection<X86_64>;
template class GotSection<X86_64>;
template class GotPltSection<X86_64>;
template class PltSection<X86_64>;
template class PltGotSection<X86_64>;
template class RelPltSection<X86_64>;
template class RelDynSection<X86_64>;
template class StrtabSection<X86_64>;
template class ShstrtabSection<X86_64>;
template class DynstrSection<X86_64>;
template class DynamicSection<X86_64>;
template class SymtabSection<X86_64>;
template class DynsymSection<X86_64>;
template class HashSection<X86_64>;
template class GnuHashSection<X86_64>;
template class MergedSection<X86_64>;
template class EhFrameSection<X86_64>;
template class EhFrameHdrSection<X86_64>;
template class DynbssSection<X86_64>;
template class VersymSection<X86_64>;
template class VerneedSection<X86_64>;
template class VerdefSection<X86_64>;
template class BuildIdSection<X86_64>;

template i64 BuildId::size(Context<X86_64> &ctx) const;
template bool is_relro(Context<X86_64> &ctx, OutputChunk<X86_64> *chunk);
template std::vector<ElfPhdr<X86_64>> create_phdr<X86_64>(Context<X86_64> &ctx);
