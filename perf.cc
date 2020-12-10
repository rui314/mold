#include "mold.h"

#include <iomanip>
#include <ios>

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
    std::cout << std::setw(20) << std::right << c->name << "=" << c->value << "\n";
}
