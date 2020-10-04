#include "catld.h"

using namespace llvm;

class ObjectFile {
public:
  ObjectFile(MemoryBufferRef m);

  void register_defined_symbols();
  void register_undefined_symbols();
 
  std::vector<Symbol *> symbols;
  bool is_alive = false;
};

