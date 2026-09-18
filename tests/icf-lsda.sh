#!/usr/bin/env bash
. $(dirname $0)/common.inc

# On 32-bit ARM, exception tables are .ARM.extab sections tied to
# functions via .ARM.exidx instead of .eh_frame FDE references, and
# ICF does not yet take them into account.
[[ $MACHINE = arm* ]] && skip

# f1 and f2 compile to identical machine code, but their exception
# tables catch different types. ICF must not merge them; if it does,
# f2 runs with f1's exception table and B escapes uncaught.

cat <<EOF | $CXX -c -o $t/a.o -O2 -ffunction-sections -xc++ -
#include <cstdio>

struct A {};
struct B {};

int which;

__attribute__((noinline)) void g() {
  if (which)
    throw B();
  throw A();
}

__attribute__((noinline)) void f1() {
  try {
    g();
  } catch (A &) {
  }
}

__attribute__((noinline)) void f2() {
  try {
    g();
  } catch (B &) {
  }
}

int main() {
  which = 1;
  f2();
  which = 0;
  f1();
  std::puts("OK");
}
EOF

$CXX -B. -o $t/exe $t/a.o -Wl,--icf=all
$QEMU $t/exe | grep OK
