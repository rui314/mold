#pragma once

#include "llvm/ADT/ArrayRef.h"
#include "llvm/ADT/SmallVector.h"
#include "llvm/ADT/Twine.h"
#include "llvm/Object/ELFTypes.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/MemoryBuffer.h"

#include <cstdlib>
#include <cstdint>
#include <string>

using llvm::ArrayRef;
using llvm::Expected;
using llvm::MemoryBufferRef;
using llvm::SmallVector;
using llvm::StringRef;
using llvm::Twine;
using llvm::object::ELF64LE;

struct Config {
  StringRef output;
};

extern Config config;

[[noreturn]] inline void error(const Twine &msg) {
  llvm::errs() << msg << "\n";
  exit(1);
}

class Symbol {
public:
};

class InputFile {
public:
};

class ObjectFile {
public:
  ObjectFile(MemoryBufferRef m);

  void register_defined_symbols();
  void register_undefined_symbols();
 
  std::vector<Symbol *> symbols;
  bool is_alive = false;
};

class InputSection {
public:
  InputSection(ObjectFile *file, ELF64LE::Shdr *hdr, StringRef name);

  void writeTo(uint8_t *buf);

  uint64_t getOffset() const;
  uint64_t getVA() const;

  InputFile *file;
};

void write();
