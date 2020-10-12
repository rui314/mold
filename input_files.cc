#include "chibild.h"

using namespace llvm;
using namespace llvm::ELF;

std::atomic_int num_symbols;
std::atomic_int num_files;

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

llvm::Timer parse1_timer("parse1", "parse1", timers);
llvm::Timer parse2_timer("parse2", "parse2", timers);

void ObjectFile::parse() {
  num_files++;

  parse1_timer.startTimer();

  ELFFile<ELF64LE> obj = check(ELFFile<ELF64LE>::create(mb.getBuffer()));
  ArrayRef<ELF64LE::Shdr> sections = CHECK(obj.sections(), this);

  // Find a symbol table.
  bool is_dso = (identify_magic(mb.getBuffer()) == file_magic::elf_shared_object);

  const ELF64LE::Shdr *symtab_sec
    = findSection(sections, is_dso ? SHT_DYNSYM : SHT_SYMTAB);

  if (!symtab_sec)
    return;

  first_global = symtab_sec->sh_info;

  ArrayRef<ELF64LE::Sym> elf_syms = CHECK(obj.symbols(symtab_sec), this);
  StringRef string_table =
    CHECK(obj.getStringTableForSymtab(*symtab_sec, sections), this);

  symbol_instances.resize(elf_syms.size());
  symbols.resize(elf_syms.size());

  parse1_timer.stopTimer();
  parse2_timer.startTimer();

  for (int i = 0; i < elf_syms.size(); i++) {
    StringRef name;
    if (first_global <= i)
      name = CHECK(elf_syms[i].getName(string_table), this);
    symbol_instances[i] = {name, this};
  }

  for (int i = 0; i < first_global; i++)
    symbols[i] = &symbol_instances[i];

  parse2_timer.stopTimer();
}

void ObjectFile::register_defined_symbols() {
  for (int i = first_global; i < symbol_instances.size(); i++) {
    Symbol &sym = symbol_instances[i];
    symbols[i] = symbol_table.add(sym.name, sym);
    num_symbols++;
  }
}

StringRef ObjectFile::getFilename() {
  return mb.getBufferIdentifier();
}

std::string toString(ObjectFile *obj) {
  return obj->getFilename().str();
}
