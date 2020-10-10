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

  firstGlobal = symtab_sec->sh_info;

  ArrayRef<ELF64LE::Sym> elf_syms = CHECK(obj.symbols(symtab_sec), this);
  StringRef string_table =
    CHECK(obj.getStringTableForSymtab(*symtab_sec, sections), this);

  symbol_instances.reserve(elf_syms.size());
  symbols.reserve(elf_syms.size());

  for (int i = 0; i < elf_syms.size(); i++) {
    StringRef name;
    if (firstGlobal <= i)
      name = CHECK(elf_syms[i].getName(string_table), this);
    symbol_instances.push_back({name, this});
  }
}

void ObjectFile::register_defined_symbols() {
  for (int i = 0; i < symbol_instances.size(); i++) {
    Symbol &sym = symbol_instances[i];

    if (i < firstGlobal) {
      symbols.push_back(&sym);
      continue;
    }

    symbol_table.add(sym.name, sym);
    llvm::errs() << symbol_table.get(sym.name).name << "\n";
    symbols.push_back(&sym);
  }
}

StringRef ObjectFile::getFilename() {
  return mb.getBufferIdentifier();
}

std::string toString(ObjectFile *obj) {
  return obj->getFilename().str();
}
