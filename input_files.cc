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

void ObjectFile::parse() {
  num_files++;

  // Initialize sections.
  ArrayRef<ELF64LE::Shdr> sections = CHECK(obj.sections(), this);
  StringRef section_strtab = CHECK(obj.getSectionStringTable(sections), this);

  this->sections.reserve(sections.size());
  for (const ELF64LE::Shdr &shdr : sections) {
    StringRef name = CHECK(obj.getSectionName(shdr, section_strtab), this);
    this->sections.push_back(new InputSection(this, &shdr, name));
  }

  // Find a symbol table.
  bool is_dso = (identify_magic(mb.getBuffer()) == file_magic::elf_shared_object);

  const ELF64LE::Shdr *symtab_sec
    = findSection(sections, is_dso ? SHT_DYNSYM : SHT_SYMTAB);

  if (!symtab_sec)
    return;

  this->first_global = symtab_sec->sh_info;

  // Parse symbols
  this->elf_syms = CHECK(obj.symbols(symtab_sec), this);
  StringRef string_table =
    CHECK(obj.getStringTableForSymtab(*symtab_sec, sections), this);

  this->symbol_instances.resize(elf_syms.size());
  this->symbols.resize(elf_syms.size());

  for (int i = 0; i < elf_syms.size(); i++) {
    StringRef name;
    if (first_global <= i)
      name = CHECK(this->elf_syms[i].getName(string_table), this);
    this->symbol_instances[i] = {name, this};
  }

  for (int i = 0; i < first_global; i++)
    this->symbols[i] = &this->symbol_instances[i];
}

void ObjectFile::register_defined_symbols() {
  for (int i = first_global; i < symbol_instances.size(); i++) {
    if (!elf_syms[i].isDefined())
      continue;

    Symbol &sym = symbol_instances[i];
    symbols[i] = symbol_table.add(sym.name, sym);
    num_defined++;
  }
}

void ObjectFile::register_undefined_symbols() {
  for (int i = first_global; i < symbol_instances.size(); i++) {
    if (elf_syms[i].isDefined())
      continue;

    Symbol &sym = symbol_instances[i];
    symbols[i] = symbol_table.get(sym.name);
    if (symbols[i])
      liveness_edges.insert(symbols[i]->file);
    num_undefined++;
  }
}

StringRef ObjectFile::get_filename() {
  return mb.getBufferIdentifier();
}

std::string toString(ObjectFile *obj) {
  StringRef s = obj->get_filename();
  if (obj->archive_name == "")
    return s.str();
  return (obj->archive_name + ":" + s).str();
}
