#include "chibild.h"

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
  elf_sections = CHECK(obj.sections(), this);
  StringRef section_strtab = CHECK(obj.getSectionStringTable(elf_sections), this);

  this->sections.reserve(sections.size());

  for (const ELF64LE::Shdr &shdr : elf_sections) {
    StringRef name = CHECK(obj.getSectionName(shdr, section_strtab), this);
    this->sections.push_back(new InputSection(this, &shdr, name));
  }
}

void ObjectFile::initialize_symbols() {
  bool is_dso = (identify_magic(mb.getBuffer()) == file_magic::elf_shared_object);

  const ELF64LE::Shdr *symtab_sec
    = findSection(elf_sections, is_dso ? SHT_DYNSYM : SHT_SYMTAB);

  if (!symtab_sec)
    return;

  this->first_global = symtab_sec->sh_info;

  // Parse symbols
  this->elf_syms = CHECK(obj.symbols(symtab_sec), this);
  StringRef string_table =
    CHECK(obj.getStringTableForSymtab(*symtab_sec, elf_sections), this);

  this->symbols.resize(elf_syms.size());

  for (int i = 0; i < elf_syms.size(); i++) {
    if (i < first_global)
      continue;
    StringRef name = CHECK(this->elf_syms[i].getName(string_table), this);
    symbols[i] = Symbol::intern(name);
  }
}

void ObjectFile::parse() {
  num_files++;
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
