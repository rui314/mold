#include "mold.h"

using namespace llvm;

std::vector<Counter *> Counter::instances;
bool Counter::enabled = true;

void Counter::print() {
  if (!enabled)
    return;

  std::vector<Counter *> vec = instances;
  std::stable_sort(vec.begin(), vec.end(), [](Counter *a, Counter *b) {
     return a->value > b->value;
  });

  for (Counter *c : vec)
    llvm::outs() << right_justify(c->name, 20) << "=" << c->value << "\n";
}
