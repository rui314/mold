#!/bin/bash
. $(dirname $0)/common.inc

# We use Intel SDE to run programs compiled for APX
command -v sde64 >& /dev/null || skip
{ sde64 -help; true; } | grep 'Diamond Rapids' || skip

cat <<EOF | $CC -o $t/a.o -c -xassembler - || skip
.globl get_foo
get_foo:
  mov %fs:0, %rax
  addq foo@gottpoff(%rip), %rax, %rax
  mov (%rax), %eax
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

_Thread_local int foo = 3;
int get_foo();

int main() {
  printf("%d %d\n", foo, get_foo());
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o
sde64 -dmr -- $t/exe1 | grep -E '^3 3$'
