#!/bin/bash
. $(dirname $0)/common.inc

# We use Intel SDE to run programs compiled for APX
command -v sde64 >& /dev/null || skip
{ sde64 -help; true; } | grep 'Diamond Rapids' || skip

cat <<EOF | $CC -o $t/a.o -c -xassembler - || skip
.globl get_foo_addr
get_foo_addr:
  mov foo@GOTPCREL(%rip), %r16
  mov %r16, %rax
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
void foo() {}
EOF

$CC -B. -o $t/c.so -shared $t/b.o

cat <<EOF | $CC -o $t/d.o -c -xc -
#include <stdio.h>

void foo();
void *get_foo_addr();

int main() {
  printf("%d %p %p\n", foo == get_foo_addr(), foo, get_foo_addr());
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/d.o
sde64 -dmr -- $t/exe1 | grep -E '^1 '
$OBJDUMP -d $t/exe1 | grep -A1 '<get_foo_addr>:' | grep -w lea

$CC -B. -o $t/exe2 $t/a.o $t/c.so $t/d.o
sde64 -dmr -- $t/exe2 | grep -E '^1 '
$OBJDUMP -d $t/exe2 | grep -A1 '<get_foo_addr>:' | grep -w mov
