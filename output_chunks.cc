#include "mold.h"

#include <shared_mutex>

using namespace llvm::ELF;

void OutputEhdr::copy_to(u8 *buf) {
  auto &hdr = *(ELF64LE::Ehdr *)(buf + shdr.sh_offset);

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

void OutputShdr::copy_to(u8 *buf) {
  u8 *base = buf + shdr.sh_offset;

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

void OutputPhdr::copy_to(u8 *buf) {
  write_vector(buf + shdr.sh_offset, create_phdr());
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
  define(DT_NULL, 0);
  return vec;
}

void DynamicSection::update_shdr() {
  shdr.sh_size = create_dynamic_section().size() * 8;
  shdr.sh_link = out::dynstr->shndx;
}

void DynamicSection::copy_to(u8 *buf) {
  write_vector(buf + shdr.sh_offset, create_dynamic_section());
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

void OutputSection::copy_to(u8 *buf) {
  if (shdr.sh_type == llvm::ELF::SHT_NOBITS)
    return;

  int num_members = members.size();

  tbb::parallel_for(0, num_members, [&](int i) {
    if (members[i]->shdr.sh_type != SHT_NOBITS) {
      // Copy section contents to an output file
      members[i]->copy_to(buf);

      // Zero-clear trailing padding
      u64 this_end = members[i]->offset + members[i]->shdr.sh_size;
      u64 next_start = (i == num_members - 1) ? shdr.sh_size : members[i + 1]->offset;
      memset(buf + shdr.sh_offset + this_end, 0, next_start - this_end);
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

void GotPltSection::initialize(u8 *buf) {
  u8 *base = buf + shdr.sh_offset;
  memset(base, 0, shdr.sh_size);
  if (out::dynamic)
    *(u64 *)base = out::dynamic->shdr.sh_addr;
}

void PltSection::initialize(u8 *buf) {
  const u8 data[] = {
    0xff, 0x35, 0, 0, 0, 0, // pushq GOTPLT+8(%rip)
    0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
    0x0f, 0x1f, 0x40, 0x00, // nop
  };

  u8 *base = buf + shdr.sh_offset;
  memcpy(base, data, sizeof(data));
  *(u32 *)(base + 2) = out::gotplt->shdr.sh_addr - shdr.sh_addr + 2;
  *(u32 *)(base + 8) = out::gotplt->shdr.sh_addr - shdr.sh_addr + 4;
}

void PltSection::write_entry(u8 *buf, Symbol *sym) {
  u8 *base = buf + shdr.sh_offset + sym->file->plt_offset + sym->plt_idx * PLT_SIZE;

  if (sym->got_idx == -1) {
    const u8 data[] = {
      0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOTPLT
      0x68, 0,    0, 0, 0,    // push  $index_in_relplt
      0xe9, 0,    0, 0, 0,    // jmp   PLT[0]
    };

    memcpy(base, data, sizeof(data));
    *(u32 *)(base + 2) = sym->get_gotplt_addr() - sym->get_plt_addr() - 6;
    *(u32 *)(base + 7) = sym->file->relplt_offset / sizeof(ELF64LE::Rela) +
      sym->relplt_idx;
    *(u32 *)(base + 12) = shdr.sh_addr - sym->get_plt_addr() - 16;
  } else {
    const u8 data[] = {
      0xff, 0x25, 0,    0,    0,    0,                   // jmp   *foo@GOTPLT
      0x66, 0x66, 0x66, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0, // nop
    };

    memcpy(base, data, sizeof(data));
    *(u32 *)(base + 2) = sym->get_got_addr() - sym->get_plt_addr() - 6;
  }
}

void DynsymSection::add_symbols(ArrayRef<Symbol *> syms) {
  for (Symbol *sym : syms) {
    sym->dynsym_idx = symbols.size() + 1;
    symbols.push_back(sym);

    sym->dynstr_offset = out::dynstr->shdr.sh_size;
    out::dynstr->shdr.sh_size += sym->name.size() + 1;
  }

  shdr.sh_size = (symbols.size() + 1) * sizeof(ELF64LE::Sym);
}

void DynsymSection::initialize(u8 *buf) {
  shdr.sh_link = out::dynstr->shndx;
  memset(buf + shdr.sh_offset, 0, sizeof(ELF64LE::Sym));
}

void DynsymSection::copy_to(u8 *buf) {
  u8 *dynsym_buf = buf + shdr.sh_offset;
  u8 *dynstr_buf = buf + out::dynstr->shdr.sh_offset;

  tbb::parallel_for_each(symbols, [&](Symbol *sym) {
    // Write to .dynsym
    auto &esym = *(ELF64LE::Sym *)(dynsym_buf + sym->dynsym_idx * sizeof(ELF64LE::Sym));
    memset(&esym, 0, sizeof(esym));
    esym.st_name = sym->dynstr_offset;
    esym.setType(sym->type);
    esym.setBinding(sym->esym->getBinding());

    if (sym->file->is_dso || sym->esym->isUndefined()) {
      esym.st_shndx = SHN_UNDEF;
    } else if (!sym->input_section) {
      esym.st_shndx = SHN_ABS;
      esym.st_value = sym->get_addr();
    } else {
      esym.st_shndx = sym->input_section->output_section->shndx;
      esym.st_value = sym->get_addr();
    }

    // Write to .dynstr
    write_string(dynstr_buf + sym->dynstr_offset, sym->name);
  });
}

void HashSection::update_shdr() {
  int header_size = 8;
  int num_slots = out::dynsym->symbols.size() + 1;
  shdr.sh_size = header_size + num_slots * 8;
  shdr.sh_link = out::dynsym->shndx;
}

void HashSection::copy_to(u8 *buf) {
  u8 *base = buf + shdr.sh_offset;
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
