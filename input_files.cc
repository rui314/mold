#include "mold.h"

using namespace llvm;
using namespace llvm::ELF;

std::atomic_int num_defined;
std::atomic_int num_undefined;
std::atomic_int num_all_syms;
std::atomic_int num_comdats;
std::atomic_int num_regular_sections;
std::atomic_int num_files;
std::atomic_int num_relocs_alloc;
std::atomic_int num_string_pieces;

ObjectFile::ObjectFile(MemoryBufferRef mb, StringRef archive_name)
  : mb(mb), archive_name(archive_name),
    obj(check(ELFFile<ELF64LE>::create(mb.getBuffer()))) {}

MemoryBufferRef readFile(StringRef path) {
  auto mbOrErr = MemoryBuffer::getFile(path, -1, false);
  if (auto ec = mbOrErr.getError())
    error("cannot open " + path + ": " + ec.message());

  std::unique_ptr<MemoryBuffer> &mb = *mbOrErr;
  MemoryBufferRef mbref = mb->getMemBufferRef();
  mb.release();
  return mbref;
}

static const ELF64LE::Shdr
*findSection(ArrayRef<ELF64LE::Shdr> sections, uint32_t type) {
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
      StringRef signature = CHECK(sym.getName(string_table), this);
      
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
      ComdatGroup *group = map.insert(signature, ComdatGroup(this, i));
      comdat_groups.push_back({group, i});
      // num_comdats++;
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
      // num_regular_sections++;
      if ((shdr.sh_flags & SHF_STRINGS) && !(shdr.sh_flags & SHF_WRITE) &&
          shdr.sh_entsize == 1) {
        read_string_pieces(shdr);
        break;
      }

      StringRef name = CHECK(obj.getSectionName(shdr, section_strtab), this);
      this->sections[i] = new InputSection(this, shdr, name);
      break;
    }
    }
  }

  for (int i = 0; i < elf_sections.size(); i++) {
    const ELF64LE::Shdr &shdr = elf_sections[i];
    if (shdr.sh_type != SHT_RELA)
      continue;

    if (shdr.sh_info >= sections.size())
      error(toString(this) + ": invalid relocated section index: " +
            Twine(shdr.sh_info));

    InputSection *target = sections[shdr.sh_info];
    if (target) {
      target->rels = CHECK(obj.relas(shdr), this);
      //      if (target->shdr.sh_flags & SHF_ALLOC)
      //        num_relocs_alloc += target->rels.size();
    }
  }
}

void ObjectFile::initialize_symbols() {
  if (!symtab_sec)
    return;

  this->symbols.resize(elf_syms.size() - first_global);

  for (int i = 0, j = first_global; j < elf_syms.size(); i++, j++) {
    StringRef name = CHECK(elf_syms[j].getName(string_table), this);
    symbols[i] = Symbol::intern(name);
  }
}

void ObjectFile::remove_comdat_members(uint32_t section_idx) {
  const ELF64LE::Shdr &shdr = elf_sections[section_idx];
  ArrayRef<ELF64LE::Word> entries =
    CHECK(obj.template getSectionContentsAsArray<ELF64LE::Word>(shdr), this);
  for (uint32_t i : entries)
    sections[i] = nullptr;
}

void ObjectFile::read_string_pieces(const ELF64LE::Shdr &shdr) {
  static ConcurrentMap<StringPiece> map1;
  static ConcurrentMap<StringPiece> map2;

  bool is_alloc = shdr.sh_type & SHF_ALLOC;
  ConcurrentMap<StringPiece> &map = is_alloc ? map1 : map2;

  ArrayRef<uint8_t> arr = CHECK(obj.getSectionContents(shdr), this);
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
  num_files++;

  bool is_dso = (identify_magic(mb.getBuffer()) == file_magic::elf_shared_object);

  elf_sections = CHECK(obj.sections(), this);
  symtab_sec = findSection(elf_sections, is_dso ? SHT_DYNSYM : SHT_SYMTAB);
  first_global = symtab_sec->sh_info;
  elf_syms = CHECK(obj.symbols(symtab_sec), this);
  string_table = CHECK(obj.getStringTableForSymtab(*symtab_sec, elf_sections), this);

  // num_all_syms += elf_syms.size();

  initialize_sections();
  initialize_symbols();
}

class Spinlock {
public:
  Spinlock(std::atomic_flag &lock) : lock(lock) {
    while (lock.test_and_set(std::memory_order_acquire));
  }

  ~Spinlock() {
    lock.clear(std::memory_order_release);
  }

private:
  std::atomic_flag &lock;  
};

void ObjectFile::register_defined_symbols() {
  for (int i = 0, j = first_global; j < elf_syms.size(); i++, j++) {
    if (!elf_syms[j].isDefined())
      continue;
    // num_defined++;

    Symbol *sym = symbols[i];
    Spinlock lock(sym->lock);

    bool is_weak = (elf_syms[i].getBinding() == STB_WEAK);

    if (!sym->file || this->priority < sym->file->priority ||
        (sym->is_weak && !is_weak)) {
      sym->file = this;
      sym->visibility = elf_syms[i].getVisibility();
      sym->is_weak = is_weak;
    }
  }
}

void ObjectFile::register_undefined_symbols() {
  if (is_alive.exchange(true))
    return;

  for (int i = 0, j = first_global; j < elf_syms.size(); i++, j++) {
    if (elf_syms[j].isDefined())
      continue;
    // num_undefined++;

    Symbol *sym = symbols[i];
    if (sym->file && sym->file->is_in_archive() && !sym->file->is_alive)
      sym->file->register_undefined_symbols();
  }
}

void ObjectFile::eliminate_duplicate_comdat_groups() {
  for (auto &pair : comdat_groups) {
    ComdatGroup *g = pair.first;
    uint32_t section_idx = pair.second;

    ObjectFile *other = g->file;
    if (other && other->priority < this->priority) {
      this->remove_comdat_members(section_idx);
      continue;      
    }

    ObjectFile *file;
    uint32_t idx;

    {
      Spinlock lock(g->lock);
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

void ObjectFile::scan_relocations() {
  for (InputSection *isec : sections)
    if (isec)
      isec->scan_relocations();
}

void ObjectFile::fix_sym_addrs() {
  for (int i = 0, j = first_global; j < elf_syms.size(); i++, j++) {
    if (symbols[i]->file != this)
      continue;
    
    InputSection *isec = sections[elf_syms[j].st_shndx];
    if (!isec) {
      llvm::outs() << "sym=" << symbols[i]->name
                   << " " << toString(symbols[i]->file)
                   << "\n";
    }
    OutputSection *osec = isec->output_section;
    symbols[i]->addr = osec->shdr.sh_addr + isec->offset + elf_syms[j].st_value;
  }
}

StringRef ObjectFile::get_filename() {
  return mb.getBufferIdentifier();
}

bool ObjectFile::is_in_archive() {
  return !archive_name.empty();
}

std::string toString(ObjectFile *obj) {
  StringRef s = obj->get_filename();
  if (obj->archive_name == "")
    return s.str();
  return (obj->archive_name + ":" + s).str();
}
