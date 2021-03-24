#include "mold.h"

#include <openssl/rand.h>
#include <openssl/sha.h>
#include <shared_mutex>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>

void OutputEhdr::copy_buf() {
  ElfEhdr &hdr = *(ElfEhdr *)(out::buf + shdr.sh_offset);
  memset(&hdr, 0, sizeof(hdr));

  memcpy(&hdr.e_ident, "\177ELF", 4);
  hdr.e_ident[EI_CLASS] = ELFCLASS64;
  hdr.e_ident[EI_DATA] = ELFDATA2LSB;
  hdr.e_ident[EI_VERSION] = EV_CURRENT;
  hdr.e_type = config.pic ? ET_DYN : ET_EXEC;
  hdr.e_machine = EM_X86_64;
  hdr.e_version = EV_CURRENT;
  if (!config.entry.empty())
    hdr.e_entry = Symbol::intern(config.entry)->get_addr();
  hdr.e_phoff = out::phdr->shdr.sh_offset;
  hdr.e_shoff = out::shdr->shdr.sh_offset;
  hdr.e_ehsize = sizeof(ElfEhdr);
  hdr.e_phentsize = sizeof(ElfPhdr);
  hdr.e_phnum = out::phdr->shdr.sh_size / sizeof(ElfPhdr);
  hdr.e_shentsize = sizeof(ElfShdr);
  hdr.e_shnum = out::shdr->shdr.sh_size / sizeof(ElfShdr);
  hdr.e_shstrndx = out::shstrtab->shndx;
}

void OutputShdr::update_shdr() {
  i64 n = 1;
  for (OutputChunk *chunk : out::chunks)
    if (chunk->kind != OutputChunk::HEADER)
      n++;
  shdr.sh_size = n * sizeof(ElfShdr);
}

void OutputShdr::copy_buf() {
  ElfShdr *hdr = (ElfShdr *)(out::buf + shdr.sh_offset);
  hdr[0] = {};

  i64 i = 1;
  for (OutputChunk *chunk : out::chunks)
    if (chunk->kind != OutputChunk::HEADER)
      hdr[i++] = chunk->shdr;
}

static i64 to_phdr_flags(OutputChunk *chunk) {
  i64 ret = PF_R;
  if (chunk->shdr.sh_flags & SHF_WRITE)
    ret |= PF_W;
  if (chunk->shdr.sh_flags & SHF_EXECINSTR)
    ret |= PF_X;
  return ret;
}

bool is_relro(OutputChunk *chunk) {
  u64 flags = chunk->shdr.sh_flags;
  u64 type = chunk->shdr.sh_type;
  std::string_view name = chunk->name;

  bool match = (flags & SHF_TLS) || type == SHT_INIT_ARRAY ||
               type == SHT_FINI_ARRAY || type == SHT_PREINIT_ARRAY ||
               chunk == out::got || chunk == out::dynamic ||
               name.ends_with(".rel.ro");

  return (flags & SHF_WRITE) && match;
}

std::vector<ElfPhdr> create_phdr() {
  std::vector<ElfPhdr> vec;

  auto define = [&](u64 type, u64 flags, i64 min_align, OutputChunk *chunk) {
    vec.push_back({});
    ElfPhdr &phdr = vec.back();
    phdr.p_type = type;
    phdr.p_flags = flags;
    phdr.p_align = std::max<u64>(min_align, chunk->shdr.sh_addralign);
    phdr.p_offset = chunk->shdr.sh_offset;
    phdr.p_filesz =
      (chunk->shdr.sh_type == SHT_NOBITS) ? 0 : chunk->shdr.sh_size;
    phdr.p_vaddr = chunk->shdr.sh_addr;
    phdr.p_memsz = chunk->shdr.sh_size;
  };

  auto append = [&](OutputChunk *chunk) {
    ElfPhdr &phdr = vec.back();
    phdr.p_align = std::max<u64>(phdr.p_align, chunk->shdr.sh_addralign);
    phdr.p_filesz = (chunk->shdr.sh_type == SHT_NOBITS)
      ? chunk->shdr.sh_offset - phdr.p_offset
      : chunk->shdr.sh_offset + chunk->shdr.sh_size - phdr.p_offset;
    phdr.p_memsz = chunk->shdr.sh_addr + chunk->shdr.sh_size - phdr.p_vaddr;
  };

  auto is_bss = [](OutputChunk *chunk) {
    return chunk->shdr.sh_type == SHT_NOBITS &&
           !(chunk->shdr.sh_flags & SHF_TLS);
  };

  // Create a PT_PHDR for the program header itself.
  define(PT_PHDR, PF_R, 8, out::phdr);

  // Create a PT_INTERP.
  if (out::interp)
    define(PT_INTERP, PF_R, 1, out::interp);

  // Create a PT_NOTE for each group of SHF_NOTE sections with the same
  // alignment requirement.
  for (i64 i = 0, end = out::chunks.size(); i < end;) {
    OutputChunk *first = out::chunks[i++];
    if (first->shdr.sh_type != SHT_NOTE)
      continue;

    i64 flags = to_phdr_flags(first);
    i64 alignment = first->shdr.sh_addralign;
    define(PT_NOTE, flags, alignment, first);

    while (i < end && out::chunks[i]->shdr.sh_type == SHT_NOTE &&
           to_phdr_flags(out::chunks[i]) == flags &&
           out::chunks[i]->shdr.sh_addralign == alignment)
      append(out::chunks[i++]);
  }

  // Create PT_LOAD segments.
  for (i64 i = 0, end = out::chunks.size(); i < end;) {
    OutputChunk *first = out::chunks[i++];
    if (!(first->shdr.sh_flags & SHF_ALLOC))
      break;

    i64 flags = to_phdr_flags(first);
    define(PT_LOAD, flags, PAGE_SIZE, first);
    first->new_page = true;

    if (!is_bss(first))
      while (i < end && !is_bss(out::chunks[i]) &&
             to_phdr_flags(out::chunks[i]) == flags)
        append(out::chunks[i++]);

    while (i < end && is_bss(out::chunks[i]) &&
           to_phdr_flags(out::chunks[i]) == flags)
      append(out::chunks[i++]);
  }

  // Create a PT_TLS.
  for (i64 i = 0; i < out::chunks.size(); i++) {
    if (!(out::chunks[i]->shdr.sh_flags & SHF_TLS))
      continue;

    define(PT_TLS, to_phdr_flags(out::chunks[i]), 1, out::chunks[i]);
    i++;
    while (i < out::chunks.size() && (out::chunks[i]->shdr.sh_flags & SHF_TLS))
      append(out::chunks[i++]);
  }

  // Add PT_DYNAMIC
  if (out::dynamic->shdr.sh_size)
    define(PT_DYNAMIC, PF_R | PF_W, 1, out::dynamic);

  // Add PT_GNU_EH_FRAME
  if (out::eh_frame_hdr)
    define(PT_GNU_EH_FRAME, PF_R, 1, out::eh_frame_hdr);

  // Add PT_GNU_STACK, which is a marker segment that doesn't really
  // contain any segments. It controls executable bit of stack area.
  vec.push_back({
    .p_type = PT_GNU_STACK,
    .p_flags = config.z_execstack ? (PF_R | PF_W | PF_X) : (PF_R | PF_W),
  });

  // Create a PT_GNU_RELRO.
  if (config.z_relro) {
    for (i64 i = 0; i < out::chunks.size(); i++) {
      if (!is_relro(out::chunks[i]))
        continue;

      define(PT_GNU_RELRO, PF_R, 1, out::chunks[i]);
      out::chunks[i]->new_page = true;
      i++;
      while (i < out::chunks.size() && is_relro(out::chunks[i]))
        append(out::chunks[i++]);
      out::chunks[i - 1]->new_page_end = true;
    }
  }

  return vec;
}

void OutputPhdr::update_shdr() {
  shdr.sh_size = create_phdr().size() * sizeof(ElfPhdr);
}

void OutputPhdr::copy_buf() {
  write_vector(out::buf + shdr.sh_offset, create_phdr());
}

void InterpSection::copy_buf() {
  write_string(out::buf + shdr.sh_offset, config.dynamic_linker);
}

void RelDynSection::update_shdr() {
  shdr.sh_link = out::dynsym->shndx;

  // .rel.dyn contents are filled by GotSection::copy_buf() and
  // InputSection::apply_reloc_alloc().
  i64 offset = out::got->get_reldyn_size();
  for (ObjectFile *file : out::objs) {
    file->reldyn_offset = offset;
    offset += file->num_dynrel * sizeof(ElfRela);
  }
  shdr.sh_size = offset;
}

void RelDynSection::sort() {
  Timer t("sort_dynamic_relocs");

  ElfRela *begin = (ElfRela *)(out::buf + shdr.sh_offset);
  ElfRela *end = (ElfRela *)(out::buf + shdr.sh_offset + shdr.sh_size);

  tbb::parallel_sort(begin, end, [](const ElfRela &a, const ElfRela &b) {
    return std::tuple(a.r_sym, a.r_offset) <
           std::tuple(b.r_sym, b.r_offset);
  });
}

void StrtabSection::update_shdr() {
  shdr.sh_size = 1;
  for (ObjectFile *file : out::objs) {
    file->strtab_offset = shdr.sh_size;
    shdr.sh_size += file->strtab_size;
  }
}

void ShstrtabSection::update_shdr() {
  shdr.sh_size = 1;
  for (OutputChunk *chunk : out::chunks) {
    if (!chunk->name.empty()) {
      chunk->shdr.sh_name = shdr.sh_size;
      shdr.sh_size += chunk->name.size() + 1;
    }
  }
}

void ShstrtabSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;
  base[0] = '\0';

  i64 i = 1;
  for (OutputChunk *chunk : out::chunks) {
    if (!chunk->name.empty()) {
      write_string(base + i, chunk->name);
      i += chunk->name.size() + 1;
    }
  }
}

i64 DynstrSection::add_string(std::string_view str) {
  if (strings.empty())
    shdr.sh_size = 1;

  auto [it, inserted] = strings.insert({str, shdr.sh_size});
  if (inserted)
    shdr.sh_size += str.size() + 1;
  return it->second;
}

i64 DynstrSection::find_string(std::string_view str) {
  auto it = strings.find(str);
  assert(it != strings.end());
  return it->second;
}

void DynstrSection::update_shdr() {
  if (shdr.sh_size == 1)
    shdr.sh_size = 0;
}

void DynstrSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;
  base[0] = '\0';

  for (std::pair<std::string_view, i64> pair : strings)
    write_string(base + pair.second, pair.first);

  if (!out::dynsym->symbols.empty()) {
    i64 offset = dynsym_offset;
    for (Symbol *sym : std::span(out::dynsym->symbols).subspan(1)) {
      write_string(base + offset, sym->name);
      offset += sym->name.size() + 1;
    }
  }
}

void SymtabSection::update_shdr() {
  shdr.sh_size = sizeof(ElfSym);

  for (ObjectFile *file : out::objs) {
    file->local_symtab_offset = shdr.sh_size;
    shdr.sh_size += file->num_local_symtab * sizeof(ElfSym);
  }

  for (ObjectFile *file : out::objs) {
    file->global_symtab_offset = shdr.sh_size;
    shdr.sh_size += file->num_global_symtab * sizeof(ElfSym);
  }

  shdr.sh_info = out::objs[0]->global_symtab_offset / sizeof(ElfSym);
  shdr.sh_link = out::strtab->shndx;

  if (shdr.sh_size == sizeof(ElfSym))
    shdr.sh_size = 0;

  static Counter counter("symtab");
  counter += shdr.sh_size / sizeof(ElfSym);
}

void SymtabSection::copy_buf() {
  memset(out::buf + shdr.sh_offset, 0, sizeof(ElfSym));
  out::buf[out::strtab->shdr.sh_offset] = '\0';

  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    file->write_symtab();
  });
}

static bool has_init_array() {
  for (OutputChunk *chunk : out::chunks)
    if (chunk->shdr.sh_type == SHT_INIT_ARRAY)
      return true;
  return false;
}

static bool has_fini_array() {
  for (OutputChunk *chunk : out::chunks)
    if (chunk->shdr.sh_type == SHT_FINI_ARRAY)
      return true;
  return false;
}

static std::vector<u64> create_dynamic_section() {
  std::vector<u64> vec;

  auto define = [&](u64 tag, u64 val) {
    vec.push_back(tag);
    vec.push_back(val);
  };

  for (SharedFile *file : out::dsos)
    define(DT_NEEDED, out::dynstr->find_string(file->soname));

  if (!config.rpaths.empty())
    define(DT_RUNPATH, out::dynstr->find_string(config.rpaths));

  if (!config.soname.empty())
    define(DT_SONAME, out::dynstr->find_string(config.soname));

  for (std::string_view str : config.auxiliary)
    define(DT_AUXILIARY, out::dynstr->find_string(str));

  for (std::string_view str : config.filter)
    define(DT_FILTER, out::dynstr->find_string(str));

  if (out::reldyn->shdr.sh_size) {
    define(DT_RELA, out::reldyn->shdr.sh_addr);
    define(DT_RELASZ, out::reldyn->shdr.sh_size);
    define(DT_RELAENT, sizeof(ElfRela));
  }

  if (out::relplt->shdr.sh_size) {
    define(DT_JMPREL, out::relplt->shdr.sh_addr);
    define(DT_PLTRELSZ, out::relplt->shdr.sh_size);
    define(DT_PLTREL, DT_RELA);
  }

  if (out::gotplt->shdr.sh_size)
    define(DT_PLTGOT, out::gotplt->shdr.sh_addr);

  if (out::dynsym->shdr.sh_size) {
    define(DT_SYMTAB, out::dynsym->shdr.sh_addr);
    define(DT_SYMENT, sizeof(ElfSym));
  }

  if (out::dynstr->shdr.sh_size) {
    define(DT_STRTAB, out::dynstr->shdr.sh_addr);
    define(DT_STRSZ, out::dynstr->shdr.sh_size);
  }

  if (has_init_array()) {
    define(DT_INIT_ARRAY, out::__init_array_start->value);
    define(DT_INIT_ARRAYSZ,
           out::__init_array_end->value - out::__init_array_start->value);
  }

  if (has_fini_array()) {
    define(DT_FINI_ARRAY, out::__fini_array_start->value);
    define(DT_FINI_ARRAYSZ,
           out::__fini_array_end->value - out::__fini_array_start->value);
  }

  if (out::versym->shdr.sh_size)
    define(DT_VERSYM, out::versym->shdr.sh_addr);

  if (out::verneed->shdr.sh_size) {
    define(DT_VERNEED, out::verneed->shdr.sh_addr);
    define(DT_VERNEEDNUM, out::verneed->shdr.sh_info);
  }

  if (out::verdef) {
    define(DT_VERDEF, out::verdef->shdr.sh_addr);
    define(DT_VERDEFNUM, out::verdef->shdr.sh_info);
  }

  if (Symbol *sym = Symbol::intern(config.init); sym->file)
    define(DT_INIT, sym->get_addr());
  if (Symbol *sym = Symbol::intern(config.fini); sym->file)
    define(DT_FINI, sym->get_addr());

  if (out::hash)
    define(DT_HASH, out::hash->shdr.sh_addr);
  if (out::gnu_hash)
    define(DT_GNU_HASH, out::gnu_hash->shdr.sh_addr);

  i64 flags = 0;
  i64 flags1 = 0;

  if (config.pie)
    flags1 |= DF_1_PIE;

  if (config.z_now) {
    flags |= DF_BIND_NOW;
    flags1 |= DF_1_NOW;
  }

  if (!config.z_dlopen)
    flags1 |= DF_1_NOOPEN;
  if (!config.z_delete)
    flags1 |= DF_1_NODELETE;

  if (out::has_gottpoff)
    flags |= DF_STATIC_TLS;

  if (flags)
    define(DT_FLAGS, flags);
  if (flags1)
    define(DT_FLAGS_1, flags1);

  define(DT_DEBUG, 0);
  define(DT_NULL, 0);
  return vec;
}

void DynamicSection::update_shdr() {
  if (config.is_static)
    return;
  if (!config.shared && out::dsos.empty())
    return;

  shdr.sh_size = create_dynamic_section().size() * 8;
  shdr.sh_link = out::dynstr->shndx;
}

void DynamicSection::copy_buf() {
  std::vector<u64> contents = create_dynamic_section();
  assert(shdr.sh_size == contents.size() * sizeof(contents[0]));
  write_vector(out::buf + shdr.sh_offset, contents);
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

OutputSection::OutputSection(std::string_view name, u32 type, u64 flags)
  : OutputChunk(REGULAR) {
  this->name = name;
  shdr.sh_type = type;
  shdr.sh_flags = flags;
  idx = instances.size();
  instances.push_back(this);
}

OutputSection *
OutputSection::get_instance(std::string_view name, u64 type, u64 flags) {
  if (name == ".eh_frame" && type == SHT_X86_64_UNWIND)
    type = SHT_PROGBITS;

  name = get_output_name(name);
  flags = flags & ~(u64)SHF_GROUP & ~(u64)SHF_COMPRESSED;

  auto find = [&]() -> OutputSection * {
    for (OutputSection *osec : OutputSection::instances)
      if (name == osec->name && type == osec->shdr.sh_type &&
          flags == osec->shdr.sh_flags)
        return osec;
    return nullptr;
  };

  static std::shared_mutex mu;

  // Search for an exiting output section.
  {
    std::shared_lock lock(mu);
    if (OutputSection *osec = find())
      return osec;
  }

  // Create a new output section.
  std::unique_lock lock(mu);
  if (OutputSection *osec = find())
    return osec;
  return new OutputSection(name, type, flags);
}

void OutputSection::copy_buf() {
  if (shdr.sh_type == SHT_NOBITS)
    return;

  tbb::parallel_for((i64)0, (i64)members.size(), [&](i64 i) {
    // Copy section contents to an output file
    InputSection &isec = *members[i];
    isec.copy_buf();

    // Zero-clear trailing padding
    u64 this_end = isec.offset + isec.shdr.sh_size;
    u64 next_start = (i == members.size() - 1) ?
      shdr.sh_size : members[i + 1]->offset;
    memset(out::buf + shdr.sh_offset + this_end, 0, next_start - this_end);
  });
}

void GotSection::add_got_symbol(Symbol *sym) {
  assert(sym->got_idx == -1);
  sym->got_idx = shdr.sh_size / GOT_SIZE;
  shdr.sh_size += GOT_SIZE;
  got_syms.push_back(sym);

  if (sym->is_imported)
    out::dynsym->add_symbol(sym);
}

void GotSection::add_gottpoff_symbol(Symbol *sym) {
  assert(sym->gottpoff_idx == -1);
  sym->gottpoff_idx = shdr.sh_size / GOT_SIZE;
  shdr.sh_size += GOT_SIZE;
  gottpoff_syms.push_back(sym);
}

void GotSection::add_tlsgd_symbol(Symbol *sym) {
  assert(sym->tlsgd_idx == -1);
  sym->tlsgd_idx = shdr.sh_size / GOT_SIZE;
  shdr.sh_size += GOT_SIZE * 2;
  tlsgd_syms.push_back(sym);
  out::dynsym->add_symbol(sym);
}

void GotSection::add_tlsdesc_symbol(Symbol *sym) {
  assert(sym->tlsdesc_idx == -1);
  sym->tlsdesc_idx = shdr.sh_size / GOT_SIZE;
  shdr.sh_size += GOT_SIZE * 2;
  tlsdesc_syms.push_back(sym);
  out::dynsym->add_symbol(sym);
}

void GotSection::add_tlsld() {
  if (tlsld_idx != -1)
    return;
  tlsld_idx = shdr.sh_size / GOT_SIZE;
  shdr.sh_size += GOT_SIZE * 2;
}

i64 GotSection::get_reldyn_size() const {
  i64 n = 0;
  for (Symbol *sym : got_syms)
    if (sym->is_imported || (config.pic && sym->is_relative()))
      n++;

  n += tlsgd_syms.size() * 2;
  n += tlsdesc_syms.size() * 2;

  for (Symbol *sym : gottpoff_syms)
    if (sym->is_imported)
      n++;

  if (tlsld_idx != -1)
    n++;

  n += out::copyrel->symbols.size();
  n += out::copyrel_relro->symbols.size();

  return n * sizeof(ElfRela);
}

// Fill .got and .rel.dyn.
void GotSection::copy_buf() {
  u64 *buf = (u64 *)(out::buf + shdr.sh_offset);
  memset(buf, 0, shdr.sh_size);

  ElfRela *rel = (ElfRela *)(out::buf + out::reldyn->shdr.sh_offset);

  for (Symbol *sym : got_syms) {
    u64 addr = sym->get_got_addr();
    if (sym->is_imported) {
      *rel++ = {addr, R_X86_64_GLOB_DAT, sym->dynsym_idx, 0};
    } else {
      buf[sym->got_idx] = sym->get_addr();
      if (config.pic && sym->is_relative())
        *rel++ = {addr, R_X86_64_RELATIVE, 0, (i64)sym->get_addr()};
    }
  }

  for (Symbol *sym : tlsgd_syms) {
    u64 addr = sym->get_tlsgd_addr();
    *rel++ = {addr, R_X86_64_DTPMOD64, sym->dynsym_idx, 0};
    *rel++ = {addr + GOT_SIZE, R_X86_64_DTPOFF64, sym->dynsym_idx, 0};
  }

  for (Symbol *sym : tlsdesc_syms)
    *rel++ = {sym->get_tlsdesc_addr(), R_X86_64_TLSDESC, sym->dynsym_idx, 0};

  for (Symbol *sym : gottpoff_syms) {
    if (sym->is_imported)
      *rel++ = {sym->get_gottpoff_addr(), R_X86_64_TPOFF32, sym->dynsym_idx, 0};
    else
      buf[sym->gottpoff_idx] = sym->get_addr() - out::tls_end;
  }

  if (tlsld_idx != -1)
    *rel++ = {get_tlsld_addr(), R_X86_64_DTPMOD64, 0, 0};

  for (Symbol *sym : out::copyrel->symbols)
    *rel++ = {sym->get_addr(), R_X86_64_COPY, sym->dynsym_idx, 0};

  for (Symbol *sym : out::copyrel_relro->symbols)
    *rel++ = {sym->get_addr(), R_X86_64_COPY, sym->dynsym_idx, 0};
}

void GotPltSection::copy_buf() {
  u64 *buf = (u64 *)(out::buf + shdr.sh_offset);

  // The first slot of .got.plt points to _DYNAMIC, as requested by
  // the x86-64 psABI. The second and the third slots are reserved by
  // the psABI.
  buf[0] = out::dynamic ? out::dynamic->shdr.sh_addr : 0;
  buf[1] = 0;
  buf[2] = 0;

  for (Symbol *sym : out::plt->symbols)
    if (sym->gotplt_idx != -1)
      buf[sym->gotplt_idx] = sym->get_plt_addr() + 6;
}

void PltSection::add_symbol(Symbol *sym) {
  assert(sym->plt_idx == -1);
  assert(sym->got_idx == -1);

  if (shdr.sh_size == 0) {
    shdr.sh_size = PLT_SIZE;
    out::gotplt->shdr.sh_size = GOT_SIZE * 3;
  }

  sym->plt_idx = shdr.sh_size / PLT_SIZE;
  shdr.sh_size += PLT_SIZE;
  symbols.push_back(sym);

  sym->gotplt_idx = out::gotplt->shdr.sh_size / GOT_SIZE;
  out::gotplt->shdr.sh_size += GOT_SIZE;
  out::relplt->shdr.sh_size += sizeof(ElfRela);
  out::dynsym->add_symbol(sym);
}

void PltSection::copy_buf() {
  u8 *buf = out::buf + shdr.sh_offset;

  static const u8 plt0[] = {
    0xff, 0x35, 0, 0, 0, 0, // pushq GOTPLT+8(%rip)
    0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
    0x0f, 0x1f, 0x40, 0x00, // nop
  };

  memcpy(buf, plt0, sizeof(plt0));
  *(u32 *)(buf + 2) = out::gotplt->shdr.sh_addr - shdr.sh_addr + 2;
  *(u32 *)(buf + 8) = out::gotplt->shdr.sh_addr - shdr.sh_addr + 4;

  i64 relplt_idx = 0;

  static const u8 data[] = {
    0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOTPLT
    0x68, 0,    0, 0, 0,    // push  $index_in_relplt
    0xe9, 0,    0, 0, 0,    // jmp   PLT[0]
  };

  for (Symbol *sym : symbols) {
    u8 *ent = buf + sym->plt_idx * PLT_SIZE;
    memcpy(ent, data, sizeof(data));
    *(u32 *)(ent + 2) = sym->get_gotplt_addr() - sym->get_plt_addr() - 6;
    *(u32 *)(ent + 7) = relplt_idx++;
    *(u32 *)(ent + 12) = shdr.sh_addr - sym->get_plt_addr() - 16;
  }
}

void PltGotSection::add_symbol(Symbol *sym) {
  assert(sym->plt_idx == -1);
  assert(sym->got_idx != -1);

  sym->plt_idx = shdr.sh_size / PLT_GOT_SIZE;
  shdr.sh_size += PLT_GOT_SIZE;
  symbols.push_back(sym);
}

void PltGotSection::copy_buf() {
  u8 *buf = out::buf + shdr.sh_offset;

  static const u8 data[] = {
    0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOT
    0x66, 0x90,             // nop
  };

  for (Symbol *sym : symbols) {
    u8 *ent = buf + sym->plt_idx * PLT_GOT_SIZE;
    memcpy(ent, data, sizeof(data));
    *(u32 *)(ent + 2) = sym->get_got_addr() - sym->get_plt_addr() - 6;
  }
}

void RelPltSection::update_shdr() {
  shdr.sh_link = out::dynsym->shndx;
}

void RelPltSection::copy_buf() {
  ElfRela *buf = (ElfRela *)(out::buf + shdr.sh_offset);
  memset(buf, 0, shdr.sh_size);

  i64 relplt_idx = 0;

  for (Symbol *sym : out::plt->symbols) {
    ElfRela &rel = buf[relplt_idx++];
    memset(&rel, 0, sizeof(rel));
    rel.r_sym = sym->dynsym_idx;
    rel.r_offset = sym->get_gotplt_addr();

    if (sym->get_type() == STT_GNU_IFUNC) {
      rel.r_type = R_X86_64_IRELATIVE;
      rel.r_addend = sym->input_section->get_addr() + sym->value;
    } else {
      rel.r_type = R_X86_64_JUMP_SLOT;
    }
  }
}

void DynsymSection::add_symbol(Symbol *sym) {
  if (symbols.empty())
    symbols.push_back({});

  if (sym->dynsym_idx != -1)
    return;
  sym->dynsym_idx = -2;
  symbols.push_back(sym);
}

void DynsymSection::sort_symbols() {
  Timer t("sort_dynsyms");

  struct T {
    Symbol *sym;
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
  shdr.sh_info = first_global - vec.begin();

  // If we have .gnu.hash section, it imposes more constraints
  // on the order of symbols.
  if (out::gnu_hash) {
    i64 num_globals = vec.end() - first_global;
    out::gnu_hash->num_buckets = num_globals / out::gnu_hash->LOAD_FACTOR + 1;
    out::gnu_hash->symoffset = first_global - vec.begin();

    tbb::parallel_for_each(first_global, vec.end(), [](T &x) {
      x.hash = gnu_hash(x.sym->name) % out::gnu_hash->num_buckets;
    });

    tbb::parallel_sort(first_global, vec.end(), [&](const T &a, const T &b) {
      return std::tuple(a.hash, a.idx) < std::tuple(b.hash, b.idx);
    });
  }

  out::dynstr->dynsym_offset = out::dynstr->shdr.sh_size;

  for (i64 i = 1; i < symbols.size(); i++) {
    symbols[i] = vec[i].sym;
    symbols[i]->dynsym_idx = i;
    out::dynstr->shdr.sh_size += symbols[i]->name.size() + 1;
  }
}

void DynsymSection::update_shdr() {
  shdr.sh_link = out::dynstr->shndx;
  shdr.sh_size = sizeof(ElfSym) * symbols.size();
}

void DynsymSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;
  memset(base, 0, sizeof(ElfSym));
  i64 name_offset = out::dynstr->dynsym_offset;

  for (i64 i = 1; i < symbols.size(); i++) {
    Symbol &sym = *symbols[i];

    ElfSym &esym = *(ElfSym *)(base + sym.dynsym_idx * sizeof(ElfSym));
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
        ? out::copyrel_relro->shndx : out::copyrel->shndx;
      esym.st_value = sym.get_addr();
    } else if (sym.file->is_dso || sym.esym->is_undef()) {
      esym.st_shndx = SHN_UNDEF;
      esym.st_size = 0;
      if (!config.shared && sym.plt_idx != -1 && sym.got_idx == -1) {
        // Emit an address for a canonical PLT
        esym.st_value = sym.get_plt_addr();
      }
    } else if (!sym.input_section) {
      esym.st_shndx = SHN_ABS;
      esym.st_value = sym.get_addr();
    } else if (sym.get_type() == STT_TLS) {
      esym.st_shndx = sym.input_section->output_section->shndx;
      esym.st_value = sym.get_addr() - out::tls_begin;
    } else {
      esym.st_shndx = sym.input_section->output_section->shndx;
      esym.st_value = sym.get_addr();
    }
  }
}

void HashSection::update_shdr() {
  if (out::dynsym->symbols.empty())
    return;

  i64 header_size = 8;
  i64 num_slots = out::dynsym->symbols.size();
  shdr.sh_size = header_size + num_slots * 8;
  shdr.sh_link = out::dynsym->shndx;
}

void HashSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;
  memset(base, 0, shdr.sh_size);

  i64 num_slots = out::dynsym->symbols.size();
  u32 *hdr = (u32 *)base;
  u32 *buckets = (u32 *)(base + 8);
  u32 *chains = buckets + num_slots;

  hdr[0] = hdr[1] = num_slots;

  for (i64 i = 1; i < out::dynsym->symbols.size(); i++) {
    Symbol *sym = out::dynsym->symbols[i];
    i64 idx = elf_hash(sym->name) % num_slots;
    chains[sym->dynsym_idx] = buckets[idx];
    buckets[idx] = sym->dynsym_idx;
  }
}

void GnuHashSection::update_shdr() {
  if (out::dynsym->symbols.empty())
    return;

  shdr.sh_link = out::dynsym->shndx;

  if (i64 num_symbols = out::dynsym->symbols.size() - symoffset) {
    // We allocate 12 bits for each symbol in the bloom filter.
    i64 num_bits = num_symbols * 12;
    num_bloom = next_power_of_two(num_bits / ELFCLASS_BITS);
  }

  i64 num_symbols = out::dynsym->symbols.size() - symoffset;

  shdr.sh_size = HEADER_SIZE;                    // Header
  shdr.sh_size += num_bloom * ELFCLASS_BITS / 8; // Bloom filter
  shdr.sh_size += num_buckets * 4;               // Hash buckets
  shdr.sh_size += num_symbols * 4;               // Hash values
}

void GnuHashSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;
  memset(base, 0, shdr.sh_size);

  *(u32 *)base = num_buckets;
  *(u32 *)(base + 4) = symoffset;
  *(u32 *)(base + 8) = num_bloom;
  *(u32 *)(base + 12) = BLOOM_SHIFT;

  std::span<Symbol *> symbols =
    std::span(out::dynsym->symbols).subspan(symoffset);

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

MergedSection *
MergedSection::get_instance(std::string_view name, u64 type, u64 flags) {
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

SectionFragment *MergedSection::insert(std::string_view data, i64 alignment) {
  assert(alignment < UINT16_MAX);

  std::string_view suffix = data;
  if (suffix.size() > 32)
    suffix = suffix.substr(suffix.size() - 32);
  i64 shard = hash_string(suffix) % NUM_SHARDS;

  MapTy::const_accessor acc;
  bool inserted =
    maps[shard].insert(acc, std::pair(data, SectionFragment(this, data)));
  SectionFragment *frag = const_cast<SectionFragment *>(&acc->second);

  update_atomic_max(frag->alignment, alignment);
  update_atomic_max(max_alignment, alignment);
  return frag;
}

void MergedSection::assign_offsets() {
  std::vector<SectionFragment *> fragments[NUM_SHARDS];
  i64 sizes[NUM_SHARDS] = {};

  tbb::parallel_for((i64)0, NUM_SHARDS, [&](i64 i) {
    for (auto it = maps[i].begin(); it != maps[i].end(); it++)
      if (SectionFragment &frag = it->second; frag.is_alive)
        fragments[i].push_back(&frag);

    // Sort section fragments to make an output deterministic.
    std::sort(fragments[i].begin(), fragments[i].end(),
              [&](SectionFragment *a, SectionFragment *b) {
                if (a->alignment != b->alignment)
                  return a->alignment > b->alignment;
                if (a->data.size() != b->data.size())
                  return a->data.size() < b->data.size();
                return a->data < b->data;
              });

    i64 offset = 0;
    for (SectionFragment *frag : fragments[i]) {
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
    for (SectionFragment *frag : fragments[i])
      frag->offset += shard_offsets[i];
  });

  shdr.sh_size = shard_offsets[NUM_SHARDS];
  shdr.sh_addralign = max_alignment;

  static Counter merged_strings("merged_strings");
  for (std::span<SectionFragment *> span : fragments)
    merged_strings += span.size();
}

void MergedSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;

  tbb::parallel_for((i64)0, NUM_SHARDS, [&](i64 i) {
    memset(base + shard_offsets[i], 0, shard_offsets[i + 1] - shard_offsets[i]);

    for (auto it = maps[i].begin(); it != maps[i].end(); it++)
      if (SectionFragment &frag = it->second; frag.is_alive)
        memcpy(base + frag.offset, frag.data.data(), frag.data.size());
  });
}

void EhFrameSection::construct() {
  // Remove dead FDEs and assign them offsets within their corresponding
  // CIE group.
  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    ObjectFile *file = out::objs[i];
    i64 count = 0;

    for (CieRecord &cie : file->cies) {
      i64 offset = 0;
      for (FdeRecord &fde : cie.fdes) {
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
  cies.reserve(out::objs.size());
  for (ObjectFile *file : out::objs)
    for (CieRecord &cie : file->cies)
      cies.push_back(&cie);

  // Record the total number of FDEs for .eh_frame_hdr.
  for (CieRecord *cie : cies) {
    cie->fde_idx = num_fdes;
    num_fdes += cie->num_fdes;
  }

  // Assign offsets within the output section to CIEs.
  auto should_merge = [](CieRecord &a, CieRecord &b) {
    return a.contents == b.contents && a.rels == b.rels;
  };

  i64 offset = 0;
  for (i64 i = 0; i < cies.size(); i++) {
    CieRecord &cie = *cies[i];
    cie.offset = offset;

    if (i == 0 || !should_merge(cie, *cies[i - 1])) {
      cie.leader_offset = offset;
      offset += cie.contents.size() + cie.fde_size;
    } else {
      cie.leader_offset = cies[i - 1]->leader_offset;
      offset += cie.fde_size;
    }
  }
  shdr.sh_size = offset;

  if (out::eh_frame_hdr)
    out::eh_frame_hdr->shdr.sh_size =
      out::eh_frame_hdr->HEADER_SIZE + num_fdes * 8;
}

void EhFrameSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;

  u8 *hdr_base = nullptr;
  if (out::eh_frame_hdr)
    hdr_base = out::buf + out::eh_frame_hdr->shdr.sh_offset;

  auto apply_reloc = [&](EhReloc &rel, u64 loc, u64 val) {
    switch (rel.type) {
    case R_X86_64_32:
      *(u32 *)(base + loc) = val;
      return;
    case R_X86_64_64:
      *(u64 *)(base + loc) = val;
      return;
    case R_X86_64_PC32:
      *(u32 *)(base + loc) = val - shdr.sh_addr - loc;
      return;
    case R_X86_64_PC64:
      *(u64 *)(base + loc) = val - shdr.sh_addr - loc;
      return;
    }
    unreachable();
  };

  struct Entry {
    i32 init_addr;
    i32 fde_addr;
  };

  // Copy CIEs and FDEs.
  tbb::parallel_for_each(cies, [&](CieRecord *cie) {
    i64 cie_size = 0;

    Entry *entry = nullptr;
    if (out::eh_frame_hdr)
      entry = (Entry *)(hdr_base + out::eh_frame_hdr->HEADER_SIZE) +
              cie->fde_idx;

    // Copy a CIE.
    if (cie->offset == cie->leader_offset) {
      memcpy(base + cie->offset, cie->contents.data(), cie->contents.size());
      cie_size = cie->contents.size();

      for (EhReloc &rel : cie->rels) {
        u64 loc = cie->offset + rel.offset;
        u64 val = rel.sym.get_addr() + rel.addend;
        apply_reloc(rel, loc, val);
      }
    }

    // Copy FDEs.
    for (FdeRecord &fde : cie->fdes) {
      if (fde.offset == -1)
        continue;

      i64 fde_off = cie->offset + cie_size + fde.offset;
      memcpy(base + fde_off, fde.contents.data(), fde.contents.size());
      *(u32 *)(base + fde_off + 4) = fde_off + 4 - cie->leader_offset;

      for (i64 i = 0; i < fde.rels.size(); i++) {
        EhReloc &rel = fde.rels[i];
        u64 loc = fde_off + rel.offset;
        u64 val = rel.sym.get_addr() + rel.addend;
        apply_reloc(rel, loc, val);

        // Write to .eh_frame_hdr
        if (out::eh_frame_hdr && i == 0) {
          assert(rel.offset == 8);
          entry->init_addr = val - out::eh_frame_hdr->shdr.sh_addr;
          entry->fde_addr =
            shdr.sh_addr + fde_off - out::eh_frame_hdr->shdr.sh_addr;
          entry++;
        }
      }
    }
  });

  if (out::eh_frame_hdr) {
    // Write .eh_frame_hdr header
    hdr_base[0] = 1;
    hdr_base[1] = DW_EH_PE_pcrel | DW_EH_PE_sdata4;
    hdr_base[2] = DW_EH_PE_udata4;
    hdr_base[3] = DW_EH_PE_datarel | DW_EH_PE_sdata4;

    *(u32 *)(hdr_base + 4) =
      shdr.sh_addr - out::eh_frame_hdr->shdr.sh_addr - 4;
    *(u32 *)(hdr_base + 8) = num_fdes;

    // Sort .eh_frame_hdr contents
    Entry *begin = (Entry *)(hdr_base + out::eh_frame_hdr->HEADER_SIZE);
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
u64 EhFrameSection::get_addr(const Symbol &sym) {
  InputSection &isec = *sym.input_section;
  const char *section_begin = isec.contents.data();

  auto contains = [](std::string_view str, const char *ptr) {
    const char *begin = str.data();
    const char *end = begin + str.size();
    return (begin == ptr) || (begin < ptr && ptr < end);
  };

  for (CieRecord &cie : isec.file.cies) {
    u64 offset = 0;

    if (cie.offset == cie.leader_offset) {
      if (contains(cie.contents, section_begin + offset)) {
        u64 cie_addr = shdr.sh_addr + cie.offset;
        u64 addend = sym.value - offset;
        return cie_addr + addend;
      }
      offset += cie.contents.size();
    }

    for (FdeRecord &fde : cie.fdes) {
      if (contains(fde.contents, section_begin + offset)) {
        if (!fde.is_alive)
          return 0;

        u64 fde_addr = shdr.sh_addr + cie.offset + offset;
        u64 addend = sym.value - offset;
        return fde_addr + addend;
      }
      offset += fde.contents.size();
    }
  }

  Fatal() << isec.file << ": .eh_frame has bad symbol: " << sym;
}

void CopyrelSection::add_symbol(Symbol *sym) {
  if (sym->has_copyrel)
    return;

  assert(!config.shared);
  assert(sym->file->is_dso);

  shdr.sh_size = align_to(shdr.sh_size, shdr.sh_addralign);
  sym->value = shdr.sh_size;
  sym->has_copyrel = true;
  shdr.sh_size += sym->esym->st_size;
  symbols.push_back(sym);
  out::dynsym->add_symbol(sym);
}

void VersymSection::update_shdr() {
  shdr.sh_size = contents.size() * sizeof(contents[0]);
  shdr.sh_link = out::dynsym->shndx;
}

void VersymSection::copy_buf() {
  write_vector(out::buf + shdr.sh_offset, contents);
}

void VerneedSection::update_shdr() {
  shdr.sh_size = contents.size();
  shdr.sh_link = out::dynstr->shndx;
}

void VerneedSection::copy_buf() {
  write_vector(out::buf + shdr.sh_offset, contents);
}

void VerdefSection::update_shdr() {
  shdr.sh_size = contents.size();
  shdr.sh_link = out::dynstr->shndx;
}

void VerdefSection::copy_buf() {
  write_vector(out::buf + shdr.sh_offset, contents);
}

void BuildIdSection::update_shdr() {
  shdr.sh_size = HEADER_SIZE + config.build_id.size();
}

void BuildIdSection::copy_buf() {
  u32 *base = (u32 *)(out::buf + shdr.sh_offset);
  memset(base, 0, shdr.sh_size);
  base[0] = 4;                      // Name size
  base[1] = config.build_id.size(); // Hash size
  base[2] = NT_GNU_BUILD_ID;        // Type
  memcpy(base + 3, "GNU", 4);       // Name string
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

void BuildIdSection::write_buildid(i64 filesize) {
  switch (config.build_id.kind) {
  case BuildId::HEX:
    write_vector(out::buf + shdr.sh_offset + HEADER_SIZE,
                 config.build_id.value);
    return;
  case BuildId::HASH: {
    // Modern x86 processors have purpose-built instructions to accelerate
    // SHA256 computation, and SHA256 outperforms MD5 on such computers.
    // So, we always compute SHA256 and truncate it if smaller digest was
    // requested.
    u8 digest[SHA256_SIZE];
    assert(config.build_id.size() <= SHA256_SIZE);
    compute_sha256(out::buf, filesize, digest);
    memcpy(out::buf + shdr.sh_offset + HEADER_SIZE, digest,
           config.build_id.size());
    return;
  }
  case BuildId::UUID:
    if (!RAND_bytes(out::buf + shdr.sh_offset + HEADER_SIZE,
                    config.build_id.size()))
      Fatal() << "RAND_bytes failed";
    return;
  }

  unreachable();
}
