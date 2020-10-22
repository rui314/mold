#include "mold.h"

using namespace llvm;
using namespace llvm::ELF;

std::atomic_int num_defined;
std::atomic_int num_undefined;
std::atomic_int num_files;

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

      static ConcurrentMap<bool> map;
      bool *handle = map.insert(signature, false);
      comdat_groups.push_back({handle, entries});
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
      StringRef name = CHECK(obj.getSectionName(shdr, section_strtab), this);
      this->sections[i] = new InputSection(this, &shdr, name);
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
    if (target)
      target->rels = CHECK(obj.relas(shdr), this);
  }
}

void ObjectFile::initialize_symbols() {
  if (!symtab_sec)
    return;

  this->symbols.resize(elf_syms.size());

  for (int i = 0; i < elf_syms.size(); i++) {
    if (i < first_global)
      continue;
    StringRef name = CHECK(this->elf_syms[i].getName(string_table), this);

    static ConcurrentMap<Symbol> map;
    symbols[i] = map.insert(name, Symbol(name));
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
  for (int i = first_global; i < symbols.size(); i++) {
    if (!elf_syms[i].isDefined())
      continue;
    num_defined++;

    Symbol *sym = symbols[i];
    Spinlock lock(sym->lock);

    if (sym->file && sym->file->priority < this->priority)
      continue;
    sym->file = this;
  }
}

void ObjectFile::register_undefined_symbols() {
  if (is_alive.exchange(true))
    return;

  for (int i = first_global; i < symbols.size(); i++) {
    if (elf_syms[i].isDefined())
      continue;
    num_undefined++;

    Symbol *sym = symbols[i];
    if (sym->file && sym->file->is_in_archive() && !sym->file->is_alive)
      sym->file->register_undefined_symbols();
  }
}

void ObjectFile::eliminate_duplicate_comdat_groups() {
  for (std::pair<bool *, ArrayRef<ELF64LE::Word>> pair : comdat_groups) {
    bool *handle = pair.first;
    ArrayRef<ELF64LE::Word> entries = pair.second;

    if (!*handle) {
      *handle = true;
      continue;
    }
    
    for (uint32_t i : entries)
      sections[i] = nullptr;
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
