#include "chibild.h"

using namespace llvm;
using namespace llvm::ELF;

ObjectFile::ObjectFile(MemoryBufferRef mb) : mb(mb) {}

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
  ELFFile<ELF64LE> obj = check(ELFFile<ELF64LE>::create(mb.getBuffer()));
  ArrayRef<ELF64LE::Shdr> sections = CHECK(obj.sections(), this);

  // Find a symbol table.
  bool is_dso = (identify_magic(mb.getBuffer()) == file_magic::elf_shared_object);

  const ELF64LE::Shdr *symtab_sec
    = findSection(sections, is_dso ? SHT_DYNSYM : SHT_SYMTAB);

  int firstGlobal = symtab_sec->sh_info;

  ArrayRef<ELF64LE::Sym> syms = CHECK(obj.symbols(symtab_sec), this);
  StringRef string_table =
    CHECK(obj.getStringTableForSymtab(*symtab_sec, sections), this);
  llvm::outs() << string_table << "\n";
}

void ObjectFile::register_defined_symbols() {}

StringRef ObjectFile::getFilename() {
  return mb.getBufferIdentifier();
}

std::string toString(ObjectFile *obj) {
  return obj->getFilename().str();
}
