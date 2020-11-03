#include "mold.h"

#include "llvm/BinaryFormat/Magic.h"

using namespace llvm;
using namespace llvm::ELF;

ObjectFile::ObjectFile(MemoryBufferRef mb, StringRef archive_name)
  : mb(mb), name(mb.getBufferIdentifier()), archive_name(archive_name),
    obj(check(ELFFile<ELF64LE>::create(mb.getBuffer()))) {}

static const ELF64LE::Shdr
*findSection(ArrayRef<ELF64LE::Shdr> sections, u32 type) {
  for (const ELF64LE::Shdr &sec : sections)
    if (sec.sh_type == type)
      return &sec;
  return nullptr;
}

void ObjectFile::initialize_sections() {
  StringRef section_strtab = CHECK(obj.getSectionStringTable(elf_sections), this);
  sections.resize(elf_sections.size());

  for (int i = 0; i < elf_sections.size(); i++) {
    const ELF64LE::Shdr &shdr = elf_sections[i];

    if ((shdr.sh_flags & SHF_EXCLUDE) && !(shdr.sh_flags & SHF_ALLOC))
      continue;

    switch (shdr.sh_type) {
    case SHT_GROUP: {
      // Get the signature of this section group.
      if (shdr.sh_info >= elf_syms.size())
        error(toString(this) + ": invalid symbol index");
      const ELF64LE::Sym &sym = elf_syms[shdr.sh_info];
      StringRef signature = CHECK(sym.getName(symbol_strtab), this);

      // Get comdat group members.
      ArrayRef<ELF64LE::Word> entries =
          CHECK(obj.template getSectionContentsAsArray<ELF64LE::Word>(shdr), this);
      if (entries.empty())
        error(toString(this) + ": empty SHT_GROUP");
      if (entries[0] == 0)
        continue;
      if (entries[0] != GRP_COMDAT)
        error(toString(this) + ": unsupported SHT_GROUP format");

      static ConcurrentMap<ComdatGroup> map;
      ComdatGroup *group = map.insert(signature, ComdatGroup(nullptr, 0));
      comdat_groups.push_back({group, i});

      static Counter counter("comdats");
      counter.inc();
      break;
    }
    case SHT_SYMTAB_SHNDX:
      error(toString(this) + ": SHT_SYMTAB_SHNDX section is not supported");
      break;
    case SHT_SYMTAB:
    case SHT_STRTAB:
    case SHT_REL:
    case SHT_RELA:
    case SHT_NULL:
      break;
    default: {
      static Counter counter("regular_sections");
      counter.inc();
#if 0
      if ((shdr.sh_flags & SHF_STRINGS) && !(shdr.sh_flags & SHF_WRITE) &&
          shdr.sh_entsize == 1) {
        read_string_pieces(shdr);
        break;
      }
#endif

      StringRef name = CHECK(obj.getSectionName(shdr, section_strtab), this);
      this->sections[i] = new InputSection(this, shdr, name);
      break;
    }
    }
  }

  for (const ELF64LE::Shdr &shdr : elf_sections) {
    if (shdr.sh_type != SHT_RELA)
      continue;

    if (shdr.sh_info >= sections.size())
      error(toString(this) + ": invalid relocated section index: " +
            Twine(shdr.sh_info));

    InputSection *target = sections[shdr.sh_info];
    if (target) {
      target->rels = CHECK(obj.relas(shdr), this);

      if (target->shdr.sh_flags & SHF_ALLOC) {
        static Counter counter("relocs_alloc");
        counter.inc(target->rels.size());
      }
    }
  }
}

void ObjectFile::initialize_symbols() {
  static Counter counter("all_syms");
  counter.inc(elf_syms.size());

  symbols.reserve(elf_syms.size());
  local_symbols.reserve(first_global);

  // First symbol entry is always null
  local_symbols.emplace_back("");
  symbols.push_back(&local_symbols.back());

  for (int i = 1; i < first_global; i++) {
    const ELF64LE::Sym &esym = elf_syms[i];
    StringRef name = CHECK(esym.getName(symbol_strtab), this);

    local_symbols.emplace_back(name);
    Symbol &sym = local_symbols.back();

    sym.file = this;
    sym.type = esym.getType();
    sym.addr = esym.st_value;

    if (esym.st_shndx != llvm::ELF::SHN_ABS) {
      if (esym.st_shndx == llvm::ELF::SHN_COMMON)
        error("common local symbol?");
      sym.input_section = sections[esym.st_shndx];
    }

    symbols.push_back(&local_symbols.back());

    if (esym.getType() != STT_SECTION) {
      local_strtab_size += name.size() + 1;
      local_symtab_size += sizeof(ELF64LE::Sym);
    }
  }

  for (int i = first_global; i < elf_syms.size(); i++) {
    const ELF64LE::Sym &esym = elf_syms[i];
    StringRef name = CHECK(esym.getName(symbol_strtab), this);
    symbols.push_back(Symbol::intern(name));

    if (esym.st_shndx == SHN_COMMON)
      has_common_symbol = true;
  }
}

void ObjectFile::remove_comdat_members(u32 section_idx) {
  const ELF64LE::Shdr &shdr = elf_sections[section_idx];
  ArrayRef<ELF64LE::Word> entries =
    CHECK(obj.template getSectionContentsAsArray<ELF64LE::Word>(shdr), this);
  for (u32 i : entries)
    sections[i] = nullptr;
}

void ObjectFile::read_string_pieces(const ELF64LE::Shdr &shdr) {
  static ConcurrentMap<StringPiece> map1;
  static ConcurrentMap<StringPiece> map2;

  bool is_alloc = shdr.sh_type & SHF_ALLOC;
  ConcurrentMap<StringPiece> &map = is_alloc ? map1 : map2;

  ArrayRef<u8> arr = CHECK(obj.getSectionContents(shdr), this);
  StringRef data((const char *)&arr[0], arr.size());

  while (!data.empty()) {
    size_t end = data.find('\0');
    if (end == StringRef::npos)
      error(toString(this) + ": string is not null terminated");

    StringRef substr = data.substr(0, end + 1);
    StringPiece *piece = map.insert(substr, StringPiece(substr));

    if (is_alloc)
      merged_strings_alloc.push_back(piece);
    else
      merged_strings_noalloc.push_back(piece);

    data = data.substr(end + 1);
    // num_string_pieces++;
  }
}

void ObjectFile::parse() {
  bool is_dso = (identify_magic(mb.getBuffer()) == file_magic::elf_shared_object);

  elf_sections = CHECK(obj.sections(), this);
  symtab_sec = findSection(elf_sections, is_dso ? SHT_DYNSYM : SHT_SYMTAB);

  if (symtab_sec) {
    first_global = symtab_sec->sh_info;
    elf_syms = CHECK(obj.symbols(symtab_sec), this);
    symbol_strtab = CHECK(obj.getStringTableForSymtab(*symtab_sec, elf_sections), this);
  }

 
  initialize_sections();
  if (symtab_sec)
    initialize_symbols();
}

void ObjectFile::register_defined_symbols() {
  for (int i = first_global; i < symbols.size(); i++) {
    const ELF64LE::Sym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];

    if (esym.isDefined()) {
      static Counter counter("defined_syms");
      counter.inc();

      InputSection *isec = nullptr;
      if (!esym.isAbsolute() && !esym.isCommon())
        isec = sections[esym.st_shndx];

      bool is_weak = (esym.getBinding() == STB_WEAK);

      std::lock_guard lock(sym.mu);

      bool is_new = !sym.file;
      bool win = sym.is_weak && !is_weak;
      bool tie_but_higher_priority =
        !is_new && !win && this->priority < sym.file->priority;

      if (is_new || win || tie_but_higher_priority) {
        sym.file = this;
        sym.input_section = isec;
        sym.addr = esym.st_value;
        sym.type = esym.getType();
        sym.visibility = esym.getVisibility();
        sym.is_weak = is_weak;
      }
    }
  }
}

void
ObjectFile::register_undefined_symbols(tbb::parallel_do_feeder<ObjectFile *> &feeder) {
  if (is_alive.exchange(true))
    return;

  for (int i = first_global; i < symbols.size(); i++) {
    const ELF64LE::Sym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];

    if (esym.isUndefined() && esym.getBinding() != STB_WEAK &&
        sym.file && sym.file->is_in_archive() && !sym.file->is_alive) {
      static Counter counter("undefined_syms");
      counter.inc();
#if 0
      llvm::outs() << toString(this) << " loads " << toString(sym.file)
                   << " for " << sym.name << "\n";
#endif
      feeder.add(sym.file);
    }
  }
}

void ObjectFile::hanlde_undefined_weak_symbols() {
  if (!is_alive)
    return;

  for (int i = first_global; i < symbols.size(); i++) {
    const ELF64LE::Sym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];

    if (esym.isUndefined() && esym.getBinding() == STB_WEAK) {
      std::lock_guard lock(sym.mu);

      bool is_new = !sym.file || !sym.file->is_alive;
      bool tie_but_higher_priority =
        !is_new && sym.is_undef_weak && this->priority < sym.file->priority;

      if (is_new || tie_but_higher_priority) {
        sym.file = this;
        sym.input_section = nullptr;
        sym.addr = 0;
        sym.visibility = esym.getVisibility();
        sym.is_undef_weak = true;
      }
    }
  }
}

void ObjectFile::eliminate_duplicate_comdat_groups() {
  for (auto &pair : comdat_groups) {
    ComdatGroup *g = pair.first;
    u32 section_idx = pair.second;

    ObjectFile *other = g->file;
    if (other && other->priority < this->priority) {
      this->remove_comdat_members(section_idx);
      continue;
    }

    ObjectFile *file;
    u32 idx;

    {
      std::lock_guard lock(g->mu);
      if (g->file == nullptr) {
        g->file = this;
        g->section_idx = section_idx;
        continue;
      }

      if (g->file.load()->priority < this->priority) {
        file = this;
        idx = section_idx;
      } else {
        file = g->file;
        idx = g->section_idx;
        g->file = this;
        g->section_idx = section_idx;
      }
    }

    file->remove_comdat_members(idx);
  }
}

void ObjectFile::convert_common_symbols() {
  if (!has_common_symbol)
    return;

  static OutputSection *bss =
    OutputSection::get_instance(".bss", SHF_WRITE | SHF_ALLOC, SHT_NOBITS);

  for (int i = first_global; i < elf_syms.size(); i++) {
    if (elf_syms[i].st_shndx != SHN_COMMON)
      continue;

    Symbol *sym = symbols[i];
    if (sym->file != this)
      continue;

    auto *shdr = new ELF64LE::Shdr;
    memset(shdr, 0, sizeof(*shdr));
    shdr->sh_flags = SHF_ALLOC;
    shdr->sh_type = SHT_NOBITS;
    shdr->sh_size = elf_syms[i].st_size;
    shdr->sh_addralign = 1;

    auto *isec = new InputSection(this, *shdr, ".bss");
    isec->output_section = bss;
    sections.push_back(isec);

    sym->input_section = isec;
    sym->addr = 0;
  }
}

void ObjectFile::fix_sym_addrs() {
  for (Symbol *sym : symbols) {
    if (sym->file != this)
      continue;

    if (InputSection *isec = sym->input_section) {
      OutputSection *osec = isec->output_section;
      sym->addr += osec->shdr.sh_addr + isec->offset;
    }
  }
}

void ObjectFile::compute_symtab() {
  for (int i = first_global; i < elf_syms.size(); i++) {
    const ELF64LE::Sym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];

    if (esym.getType() != STT_SECTION && sym.file == this) {
      global_strtab_size += sym.name.size() + 1;
      global_symtab_size += sizeof(ELF64LE::Sym);
    }
  }
}

void ObjectFile::write_symtab(u8 *buf, u64 symtab_off, u64 strtab_off,
                              u32 start, u32 end) {
  u8 *symtab = buf + out::symtab->shdr.sh_offset;
  u8 *strtab = buf + out::strtab->shdr.sh_offset;

  for (int i = start; i < end; i++) {
    Symbol &sym = *symbols[i];
    if (sym.type == STT_SECTION || sym.file != this)
      continue;

    auto &esym = *(ELF64LE::Sym *)(symtab + symtab_off);
    esym.st_name = strtab_off;
    esym.st_value = sym.addr;
    esym.st_size = elf_syms[i].st_size;
    esym.st_info = elf_syms[i].st_info;
    esym.st_shndx = sym.input_section
      ? sym.input_section->output_section->idx : SHN_ABS;

    symtab_off += sizeof(ELF64LE::Sym);

    memcpy(strtab + strtab_off, sym.name.data(), sym.name.size());
    strtab_off += sym.name.size() + 1;
  }
}

void ObjectFile::write_local_symtab(u8 *buf, u64 symtab_off, u64 strtab_off) {
  write_symtab(buf, symtab_off, strtab_off, 1, first_global);
}

void ObjectFile::write_global_symtab(u8 *buf, u64 symtab_off, u64 strtab_off) {
  write_symtab(buf, symtab_off, strtab_off, first_global, elf_syms.size());
}

bool ObjectFile::is_in_archive() {
  return !archive_name.empty();
}

ObjectFile *ObjectFile::create_internal_file() {
  // Create a dummy object file.
  constexpr int bufsz = 256;
  char *buf = new char[bufsz];
  std::unique_ptr<MemoryBuffer> mb =
    MemoryBuffer::getMemBuffer(StringRef(buf, bufsz));

  auto *obj = new ObjectFile(mb->getMemBufferRef(), "");
  obj->name = "<internal>";
  mb.release();

  // Create linker-synthesized symbols.
  auto *elf_syms = new std::vector<ELF64LE::Sym>;
  obj->elf_syms = *elf_syms;

  auto add = [&](StringRef name) {
    Symbol *sym = Symbol::intern(name);
    sym->file = obj;
    obj->symbols.push_back(sym);

    ELF64LE::Sym esym = {};
    esym.setType(STT_NOTYPE);
    esym.setBinding(STB_GLOBAL);
    elf_syms->push_back(esym);
    return sym;
  };

  out::__bss_start = add("__bss_start");
  out::__ehdr_start = add("__ehdr_start");
  out::__rela_iplt_start = add("__rela_iplt_start");
  out::__rela_iplt_end = add("__rela_iplt_end");
  return obj;
}

std::string toString(ObjectFile *obj) {
  StringRef s = obj->name;
  if (obj->archive_name == "")
    return s.str();
  return (obj->archive_name + ":" + s).str();
}
