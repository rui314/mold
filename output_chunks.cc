#include "mold.h"

#include <shared_mutex>

using namespace llvm::ELF;

void OutputEhdr::copy_buf() {
  auto &hdr = *(ELF64LE::Ehdr *)(out::buf + shdr.sh_offset);

  memset(&hdr, 0, sizeof(hdr));
  memcpy(&hdr.e_ident, "\177ELF", 4);
  hdr.e_ident[EI_CLASS] = ELFCLASS64;
  hdr.e_ident[EI_DATA] = ELFDATA2LSB;
  hdr.e_ident[EI_VERSION] = EV_CURRENT;
  hdr.e_ident[EI_OSABI] = 0;
  hdr.e_ident[EI_ABIVERSION] = 0;
  hdr.e_type = ET_EXEC;
  hdr.e_machine = EM_X86_64;
  hdr.e_version = EV_CURRENT;
  hdr.e_entry = Symbol::intern("_start")->get_addr();
  hdr.e_phoff = out::phdr->shdr.sh_offset;
  hdr.e_shoff = out::shdr->shdr.sh_offset;
  hdr.e_flags = 0;
  hdr.e_ehsize = sizeof(ELF64LE::Ehdr);
  hdr.e_phentsize = sizeof(ELF64LE::Phdr);
  hdr.e_phnum = out::phdr->shdr.sh_size / sizeof(ELF64LE::Phdr);
  hdr.e_shentsize = sizeof(ELF64LE::Shdr);
  hdr.e_shnum = out::shdr->shdr.sh_size / sizeof(ELF64LE::Shdr);
  hdr.e_shstrndx = out::shstrtab->shndx;
}

void OutputShdr::update_shdr() {
  int i = 1;
  for (OutputChunk *chunk : out::chunks)
    if (chunk->kind != OutputChunk::HEADER)
      i++;
  shdr.sh_size = i * sizeof(ELF64LE::Shdr);
}

void OutputShdr::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;

  memset(base, 0, sizeof(ELF64LE::Shdr));

  auto *ptr = (ELF64LE::Shdr *)(base + sizeof(ELF64LE::Shdr));
  for (OutputChunk *chunk : out::chunks)
    if (chunk->kind != OutputChunk::HEADER)
      *ptr++ = chunk->shdr;
}

static u32 to_phdr_flags(OutputChunk *chunk) {
  u32 ret = PF_R;
  if (chunk->shdr.sh_flags & SHF_WRITE)
    ret |= PF_W;
  if (chunk->shdr.sh_flags & SHF_EXECINSTR)
    ret |= PF_X;
  return ret;
}

static std::vector<ELF64LE::Phdr> create_phdr() {
  std::vector<ELF64LE::Phdr> vec;

  auto define = [&](u32 type, u32 flags, u32 align, OutputChunk *chunk) {
    vec.push_back({});
    ELF64LE::Phdr &phdr = vec.back();
    phdr.p_type = type;
    phdr.p_flags = flags;
    phdr.p_align = std::max<u64>(align, chunk->shdr.sh_addralign);
    phdr.p_offset = chunk->shdr.sh_offset;
    phdr.p_filesz = (chunk->shdr.sh_type == SHT_NOBITS) ? 0 : chunk->shdr.sh_size;
    phdr.p_vaddr = chunk->shdr.sh_addr;
    phdr.p_memsz = chunk->shdr.sh_size;

    if (type == PT_LOAD)
      chunk->starts_new_ptload = true;
  };

  auto append = [&](OutputChunk *chunk) {
    ELF64LE::Phdr &phdr = vec.back();
    phdr.p_align = std::max<u64>(phdr.p_align, chunk->shdr.sh_addralign);
    phdr.p_filesz = (chunk->shdr.sh_type == SHT_NOBITS)
      ? chunk->shdr.sh_offset - phdr.p_offset
      : chunk->shdr.sh_offset + chunk->shdr.sh_size - phdr.p_offset;
    phdr.p_memsz = chunk->shdr.sh_addr + chunk->shdr.sh_size - phdr.p_vaddr;
  };

  auto is_bss = [](OutputChunk *chunk) {
    return chunk->shdr.sh_type == SHT_NOBITS && !(chunk->shdr.sh_flags & SHF_TLS);
  };

  // Create a PT_PHDR for the program header itself.
  define(PT_PHDR, PF_R, 8, out::phdr);

  // Create an PT_INTERP.
  if (out::interp)
    define(PT_INTERP, PF_R, 1, out::interp);

  // Create PT_LOAD segments.
  for (int i = 0, end = out::chunks.size(); i < end;) {
    OutputChunk *first = out::chunks[i++];
    if (!(first->shdr.sh_flags & SHF_ALLOC))
      break;

    u32 flags = to_phdr_flags(first);
    define(PT_LOAD, flags, PAGE_SIZE, first);

    if (!is_bss(first))
      while (i < end && !is_bss(out::chunks[i]) &&
             to_phdr_flags(out::chunks[i]) == flags)
        append(out::chunks[i++]);

    while (i < end && is_bss(out::chunks[i]) &&
           to_phdr_flags(out::chunks[i]) == flags)
      append(out::chunks[i++]);
  }

  // Create a PT_TLS.
  for (int i = 0; i < out::chunks.size(); i++) {
    if (out::chunks[i]->shdr.sh_flags & SHF_TLS) {
      define(PT_TLS, to_phdr_flags(out::chunks[i]), 1, out::chunks[i]);
      i++;
      while (i < out::chunks.size() && (out::chunks[i]->shdr.sh_flags & SHF_TLS))
        append(out::chunks[i++]);
    }
  }

  // Add PT_DYNAMIC
  if (out::dynamic)
    define(PT_DYNAMIC, PF_R | PF_W, out::dynamic->shdr.sh_addralign, out::dynamic);

  return vec;
}

void OutputPhdr::update_shdr() {
  shdr.sh_size = create_phdr().size() * sizeof(ELF64LE::Phdr);
}

void OutputPhdr::copy_buf() {
  write_vector(out::buf + shdr.sh_offset, create_phdr());
}

void InterpSection::copy_buf() {
  write_string(out::buf + shdr.sh_offset, config.dynamic_linker);
}

void RelDynSection::update_shdr() {
  shdr.sh_link = out::dynsym->shndx;

  auto count = [](ArrayRef<Symbol *> syms) {
    int i = 0;
    return i;
  };

  for (Symbol *sym : out::got->got_syms)
    if (sym->is_imported)
      shdr.sh_size += sizeof(ELF64LE::Rela);

  for (Symbol *sym : out::got->tlsgd_syms)
    shdr.sh_size += sizeof(ELF64LE::Rela) * 2;

  for (Symbol *sym : out::got->tlsld_syms)
    shdr.sh_size += sizeof(ELF64LE::Rela);
}

void RelDynSection::copy_buf() {
  ELF64LE::Rela *rel = (ELF64LE::Rela *)(out::buf + shdr.sh_offset);

  for (Symbol *sym : out::got->got_syms) {
    if (!sym->is_imported)
      continue;

    memset(rel, 0, sizeof(*rel));
    rel->setSymbol(sym->dynsym_idx, false);
    rel->setType(R_X86_64_GLOB_DAT, false);
    rel->r_offset = sym->get_got_addr();
    rel++;
  }

  for (Symbol *sym : out::got->tlsgd_syms) {
    memset(rel, 0, sizeof(*rel));
    rel->setSymbol(sym->is_imported ? sym->dynsym_idx : 0, false);
    rel->setType(R_X86_64_DTPMOD64, false);
    rel->r_offset = sym->get_tlsgd_addr();
    rel++;

    memset(rel, 0, sizeof(*rel));
    rel->setSymbol(sym->is_imported ? sym->dynsym_idx : 0, false);
    rel->setType(R_X86_64_DTPOFF64, false);
    rel->r_offset = sym->get_tlsgd_addr() + GOT_SIZE;
    rel++;
  }

  for (Symbol *sym : out::got->tlsld_syms) {
    memset(rel, 0, sizeof(*rel));
    rel->setSymbol(sym->is_imported ? sym->dynsym_idx : 0, false);
    rel->setType(R_X86_64_DTPMOD64, false);
    rel->r_offset = sym->get_tlsld_addr();
    rel++;
  }
}

void StrtabSection::initialize_buf() {
  out::buf[shdr.sh_offset] = '\0';
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

  int i = 1;
  for (OutputChunk *chunk : out::chunks) {
    if (!chunk->name.empty()) {
      write_string(base + i, chunk->name);
      i += chunk->name.size() + 1;
    }
  }
}

u32 DynstrSection::add_string(StringRef str) {
  u32 ret = shdr.sh_size;
  shdr.sh_size += str.size() + 1;
  contents.push_back(str);
  return ret;
}

void DynstrSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;
  base[0] = '\0';

  int i = 1;
  for (StringRef s : contents) {
    write_string(base + i, s);
    i += s.size() + 1;
  }
}

void SymtabSection::update_shdr() {
  shdr.sh_link = out::strtab->shndx;

  tbb::parallel_for_each(out::files,
                         [](ObjectFile *file) { file->compute_symtab(); });

  local_symtab_off.resize(out::files.size() + 1);
  local_strtab_off.resize(out::files.size() + 1);
  global_symtab_off.resize(out::files.size() + 1);
  global_strtab_off.resize(out::files.size() + 1);

  local_symtab_off[0] = sizeof(ELF64LE::Sym);
  local_strtab_off[0] = 1;

  for (int i = 1; i < out::files.size() + 1; i++) {
    local_symtab_off[i] = local_symtab_off[i - 1] + out::files[i - 1]->local_symtab_size;
    local_strtab_off[i] = local_strtab_off[i - 1] + out::files[i - 1]->local_strtab_size;
  }

  shdr.sh_info = local_symtab_off.back() / sizeof(ELF64LE::Sym);

  global_symtab_off[0] = local_symtab_off.back();
  global_strtab_off[0] = local_strtab_off.back();

  for (int i = 1; i < out::files.size() + 1; i++) {
    global_symtab_off[i] =
      global_symtab_off[i - 1] + out::files[i - 1]->global_symtab_size;
    global_strtab_off[i] =
      global_strtab_off[i - 1] + out::files[i - 1]->global_strtab_size;
  }

  shdr.sh_size = global_symtab_off.back();
  out::strtab->shdr.sh_size = global_strtab_off.back();
}

void SymtabSection::copy_buf() {
  memset(out::buf + shdr.sh_offset, 0, sizeof(ELF64LE::Sym));

  tbb::parallel_for((size_t)0, out::files.size(), [&](size_t i) {
    out::files[i]->write_local_symtab(local_symtab_off[i], local_strtab_off[i]);
    out::files[i]->write_global_symtab(global_symtab_off[i], global_strtab_off[i]);
  });
}

static std::vector<u64> create_dynamic_section() {
  std::vector<u64> vec;

  auto define = [&](u64 tag, u64 val) {
    vec.push_back(tag);
    vec.push_back(val);
  };

  int i = 1;
  for (ObjectFile *file : out::files) {
    if (!file->soname.empty()) {
      define(DT_NEEDED, i);
      i += file->soname.size() + 1;
    }
  }

  define(DT_RELA, out::reldyn->shdr.sh_addr);
  define(DT_RELASZ, out::reldyn->shdr.sh_size);
  define(DT_RELAENT, sizeof(ELF64LE::Rela));
  define(DT_JMPREL, out::relplt->shdr.sh_addr);
  define(DT_PLTRELSZ, out::relplt->shdr.sh_size);
  define(DT_PLTGOT, out::gotplt->shdr.sh_addr);
  define(DT_PLTREL, DT_RELA);
  define(DT_SYMTAB, out::dynsym->shdr.sh_addr);
  define(DT_SYMENT, sizeof(ELF64LE::Sym));
  define(DT_STRTAB, out::dynstr->shdr.sh_addr);
  define(DT_STRSZ, out::dynstr->shdr.sh_size);
  define(DT_HASH, out::hash->shdr.sh_addr);
  define(DT_INIT_ARRAY, out::__init_array_start->value);
  define(DT_INIT_ARRAYSZ, out::__init_array_end->value - out::__init_array_start->value);
  define(DT_FINI_ARRAY, out::__fini_array_start->value);
  define(DT_FINI_ARRAYSZ, out::__fini_array_end->value - out::__fini_array_start->value);

  auto find = [](StringRef name) -> OutputChunk * {
    for (OutputChunk *chunk : out::chunks)
      if (chunk->name == name)
        return chunk;
    return nullptr;
  };

  if (OutputChunk *chunk = find(".init"))
    define(DT_INIT, chunk->shdr.sh_addr);
  if (OutputChunk *chunk = find(".fini"))
    define(DT_FINI, chunk->shdr.sh_addr);

  define(DT_NULL, 0);
  return vec;
}

void DynamicSection::update_shdr() {
  shdr.sh_size = create_dynamic_section().size() * 8;
  shdr.sh_link = out::dynstr->shndx;
}

void DynamicSection::copy_buf() {
  write_vector(out::buf + shdr.sh_offset, create_dynamic_section());
}

static StringRef get_output_name(StringRef name) {
  static StringRef common_names[] = {
    ".text.", ".data.rel.ro.", ".data.", ".rodata.", ".bss.rel.ro.",
    ".bss.", ".init_array.", ".fini_array.", ".tbss.", ".tdata.",
  };

  for (StringRef s : common_names)
    if (name.startswith(s) || name == s.drop_back())
      return s.drop_back();
  return name;
}

OutputSection *
OutputSection::get_instance(StringRef name, u64 flags, u32 type) {
  name = get_output_name(name);
  flags = flags & ~(u64)SHF_GROUP;

  auto find = [&]() -> OutputSection * {
    for (OutputSection *osec : OutputSection::instances)
      if (name == osec->name && type == osec->shdr.sh_type &&
          flags == (osec->shdr.sh_flags & ~SHF_GROUP))
        return osec;
    return nullptr;
  };

  // Search for an exiting output section.
  static std::shared_mutex mu;
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
  if (shdr.sh_type == llvm::ELF::SHT_NOBITS)
    return;

  int num_members = members.size();

  tbb::parallel_for(0, num_members, [&](int i) {
    if (members[i]->shdr.sh_type != SHT_NOBITS) {
      // Copy section contents to an output file
      members[i]->copy_buf();

      // Zero-clear trailing padding
      u64 this_end = members[i]->offset + members[i]->shdr.sh_size;
      u64 next_start = (i == num_members - 1) ? shdr.sh_size : members[i + 1]->offset;
      memset(out::buf + shdr.sh_offset + this_end, 0, next_start - this_end);
    }
  });
}

bool OutputSection::empty() const {
  if (!members.empty())
    for (InputChunk *mem : members)
      if (mem->shdr.sh_size)
        return false;
  return true;
}

void GotSection::add_symbol(Symbol *sym) {
  sym->got_idx = shdr.sh_size / GOT_SIZE;
  shdr.sh_size += GOT_SIZE;
  got_syms.push_back(sym);

  if (sym->is_imported)
    out::dynsym->add_symbol(sym);
}

void GotSection::add_gottpoff_symbol(Symbol *sym) {
  sym->gottpoff_idx = shdr.sh_size / GOT_SIZE;
  shdr.sh_size += GOT_SIZE;
  gottpoff_syms.push_back(sym);
}

void GotSection::add_tlsgd_symbol(Symbol *sym) {
  sym->tlsgd_idx = shdr.sh_size / GOT_SIZE;
  shdr.sh_size += GOT_SIZE * 2;
  tlsgd_syms.push_back(sym);

  if (sym->is_imported)
    out::dynsym->add_symbol(sym);
}

void GotSection::add_tlsld_symbol(Symbol *sym) {
  sym->tlsld_idx = shdr.sh_size / GOT_SIZE;
  shdr.sh_size += GOT_SIZE * 2;
  tlsld_syms.push_back(sym);

  if (sym->is_imported)
    out::dynsym->add_symbol(sym);
}

void GotSection::copy_buf() {
  u64 *buf = (u64 *)(out::buf + shdr.sh_offset);
  memset(buf, 0, shdr.sh_size);

  for (Symbol *sym : got_syms)
    if (!sym->is_imported)
      buf[sym->got_idx] = sym->get_addr();

  for (Symbol *sym : gottpoff_syms)
    buf[sym->gottpoff_idx] = sym->get_addr() - out::tls_end;
}

void GotPltSection::copy_buf() {
  u64 *buf = (u64 *)(out::buf + shdr.sh_offset);

  buf[0] = out::dynamic ? out::dynamic->shdr.sh_addr : 0;
  buf[1] = 0;
  buf[2] = 0;

  for (Symbol *sym : out::plt->symbols)
    if (sym->gotplt_idx != -1)
      buf[sym->gotplt_idx] = sym->get_plt_addr() + 6;
}

void PltSection::add_symbol(Symbol *sym) {
  sym->plt_idx = shdr.sh_size / PLT_SIZE;
  shdr.sh_size += PLT_SIZE;
  symbols.push_back(sym);

  if (sym->got_idx == -1) {
    sym->gotplt_idx = out::gotplt->shdr.sh_size / GOT_SIZE;
    out::gotplt->shdr.sh_size += GOT_SIZE;

    sym->relplt_idx = out::relplt->shdr.sh_size / sizeof(ELF64LE::Rela);
    out::relplt->shdr.sh_size += sizeof(ELF64LE::Rela);

    out::dynsym->add_symbol(sym);
  }
}

void PltSection::copy_buf() {
  u8 *buf = out::buf + shdr.sh_offset;

  const u8 plt0[] = {
    0xff, 0x35, 0, 0, 0, 0, // pushq GOTPLT+8(%rip)
    0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
    0x0f, 0x1f, 0x40, 0x00, // nop
  };

  memcpy(buf, plt0, sizeof(plt0));
  *(u32 *)(buf + 2) = out::gotplt->shdr.sh_addr - shdr.sh_addr + 2;
  *(u32 *)(buf + 8) = out::gotplt->shdr.sh_addr - shdr.sh_addr + 4;

  for (Symbol *sym : symbols) {
    u8 *ent = buf + sym->plt_idx * PLT_SIZE;

    if (sym->got_idx == -1) {
      const u8 data[] = {
        0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOTPLT
        0x68, 0,    0, 0, 0,    // push  $index_in_relplt
        0xe9, 0,    0, 0, 0,    // jmp   PLT[0]
      };

      memcpy(ent, data, sizeof(data));
      *(u32 *)(ent + 2) = sym->get_gotplt_addr() - sym->get_plt_addr() - 6;
      *(u32 *)(ent + 7) = sym->relplt_idx;
      *(u32 *)(ent + 12) = shdr.sh_addr - sym->get_plt_addr() - 16;
    } else {
      const u8 data[] = {
        0xff, 0x25, 0,    0,    0,    0,                   // jmp   *foo@GOT
        0x66, 0x66, 0x66, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0, // nop
      };

      memcpy(ent, data, sizeof(data));
      *(u32 *)(ent + 2) = sym->get_got_addr() - sym->get_plt_addr() - 6;
    }
  }
}

void RelPltSection::update_shdr() {
  shdr.sh_link = out::dynsym->shndx;
}

void RelPltSection::copy_buf() {
  ELF64LE::Rela *buf = (ELF64LE::Rela *)(out::buf + shdr.sh_offset);

  for (Symbol *sym : out::plt->symbols) {
    if (sym->relplt_idx == -1)
      continue;

    ELF64LE::Rela &rel = buf[sym->relplt_idx];
    memset(&rel, 0, sizeof(rel));
    rel.setSymbol(sym->dynsym_idx, false);
    rel.r_offset = sym->get_gotplt_addr();

    if (sym->type == STT_GNU_IFUNC) {
      rel.setType(R_X86_64_IRELATIVE, false);
      rel.r_addend = sym->get_addr();
    } else {
      rel.setType(R_X86_64_JUMP_SLOT, false);
    }
  }
}

void DynsymSection::add_symbol(Symbol *sym) {
  if (sym->dynsym_idx == -1) {
    sym->dynsym_idx = symbols.size() + 1;
    sym->dynstr_offset = out::dynstr->add_string(sym->name);
    symbols.push_back(sym);
  }
}

void DynsymSection::update_shdr() {
  shdr.sh_link = out::dynstr->shndx;
  shdr.sh_size = sizeof(ELF64LE::Sym) * (symbols.size() + 1);
}

void DynsymSection::initialize_buf() {
  memset(out::buf + shdr.sh_offset, 0, sizeof(ELF64LE::Sym));
}

void DynsymSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;

  tbb::parallel_for_each(symbols, [&](Symbol *sym) {
    auto &esym = *(ELF64LE::Sym *)(base + sym->dynsym_idx * sizeof(ELF64LE::Sym));
    memset(&esym, 0, sizeof(esym));
    esym.st_name = sym->dynstr_offset;
    esym.setType(sym->type);
    esym.setBinding(sym->esym->getBinding());

    if (sym->is_imported || sym->esym->isUndefined()) {
      esym.st_shndx = SHN_UNDEF;
    } else if (!sym->input_section) {
      esym.st_shndx = SHN_ABS;
      esym.st_value = sym->get_addr();
    } else {
      esym.st_shndx = sym->input_section->output_section->shndx;
      esym.st_value = sym->get_addr();
    }
  });
}

void HashSection::update_shdr() {
  int header_size = 8;
  int num_slots = out::dynsym->symbols.size() + 1;
  shdr.sh_size = header_size + num_slots * 8;
  shdr.sh_link = out::dynsym->shndx;
}

void HashSection::copy_buf() {
  u8 *base = out::buf + shdr.sh_offset;
  memset(base, 0, shdr.sh_size);

  int num_slots = out::dynsym->symbols.size() + 1;
  u32 *hdr = (u32 *)base;
  u32 *buckets = (u32 *)(base + 8);
  u32 *chains = buckets + num_slots;

  hdr[0] = hdr[1] = num_slots;

  for (Symbol *sym : out::dynsym->symbols) {
    u32 i = hash(sym->name) % num_slots;
    chains[sym->dynsym_idx] = buckets[i];
    buckets[i] = sym->dynsym_idx;
  }
}

u32 HashSection::hash(StringRef name) {
  u32 h = 0;
  for (char c : name) {
    h = (h << 4) + c;
    u32 g = h & 0xf0000000;
    if (g != 0)
      h ^= g >> 24;
    h &= ~g;
  }
  return h;
}

MergedSection *
MergedSection::get_instance(StringRef name, u64 flags, u32 type) {
  name = get_output_name(name);
  flags = flags & ~(u64)SHF_MERGE & ~(u64)SHF_STRINGS;

  auto find = [&]() -> MergedSection * {
    for (MergedSection *osec : MergedSection::instances)
      if (name == osec->name && flags == osec->shdr.sh_flags &&
          type == osec->shdr.sh_type)
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
