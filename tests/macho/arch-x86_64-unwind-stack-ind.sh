#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = x86_64 ] || skip

# A frame of more than 2040 bytes without a frame pointer gets an x86-64
# compact unwind encoding in "stack immediate indirect" mode, which says
# where in the function the instruction holding the stack size is, as an
# offset from the start of its __unwind_info entry. So two adjacent
# functions with the same encoding still need an entry each; merged, the
# second would be unwound with the first one's stack size, and an
# exception thrown through it would never reach its handler.
cat <<EOF | $CXX -o $t/a.o -c -xc++ - -O2 -fomit-frame-pointer
#include <cstdio>
extern "C" void use(char *, int);
extern "C" __attribute__((noinline)) void f1(int n) {
  char buf[2500]; use(buf, n); use(buf, n + 1);
}
extern "C" __attribute__((noinline)) void f2(int n) {
  char buf[3500];
  for (int i = 0; i < 3500; i++) buf[i] = 0x55;
  use(buf, n); use(buf, n + 1);
}
int main() {
  f1(5);
  try { f2(2); } catch (int e) { printf("caught %d\n", e); }
}
EOF

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
extern "C" void use(char *p, int n) { p[n] = 1; if (n == 2) throw 42; }
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$RUN $t/exe | grep -q '^caught 42$'
