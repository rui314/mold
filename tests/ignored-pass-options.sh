#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime takes ld64's switches for passes it doesn't run - the labels
# of FDEs in a -r output, the ordering of initializers, x86-64's pass
# for huge zero-fill sections - without a word, and makes the same
# output with them as without.
cat <<EOF | $CXX -o $t/a.o -c -xc++ -
#include <stdio.h>
#include <stdexcept>
static char big[1 << 20];
struct S { S() { puts("ctor"); } } s;
__attribute__((constructor)) void init1() { puts("init1"); }
int main(int argc, char **argv) {
  try { if (argc) throw std::runtime_error("x"); } catch (std::exception &e) { puts(e.what()); }
  return big[argc];
}
EOF

for opt in -no_eh_labels -no_order_inits -no_huge; do
  $CXX --ld-path=$mold -o $t/exe $t/a.o
  mv $t/exe $t/exe1
  $CXX --ld-path=$mold -o $t/exe $t/a.o -Wl,$opt 2> $t/log
  [ ! -s $t/log ]
  cmp $t/exe $t/exe1

  $mold -r -arch $ARCH -o $t/r1.o $t/a.o
  $mold -r -arch $ARCH -o $t/r2.o $t/a.o $opt
  cmp $t/r1.o $t/r2.o
done
