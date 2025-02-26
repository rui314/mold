#!/bin/bash
. $(dirname $0)/common.inc

supports_tlsdesc || skip

# We use Intel SDE to run programs compiled for APX
command -v sde64 >& /dev/null || skip
{ sde64 -help; true; } | grep 'Diamond Rapids' || skip

cat <<EOF | $GCC -c -o $t/a.o -xassembler - || skip
.globl  get_foo
.type   get_foo, @function
get_foo:
  pushq   %rbp
  movq    %rsp, %rbp
  leaq    foo@TLSDESC(%rip), %r16
  movq    %r16, %rax
  call    *foo@TLSCALL(%rax)
  movq    %fs:0, %rdx
  addq    %rdx, %rax
  movl    (%rax), %eax
  popq    %rbp
  ret
EOF

cat <<EOF | $GCC -fPIC -c -o $t/b.o -xc - $tlsdesc_opt
#include <stdio.h>

_Thread_local int foo;

int get_foo();

int main() {
  foo = 42;
  printf("%d\n", get_foo());
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o
sde64 -dmr -- $t/exe1 | grep 42

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,-no-relax
sde64 -dmr -- $t/exe2 | grep 42

$CC -shared -o $t/c.so $t/a.o
$CC -B. -o $t/exe3 $t/b.o $t/c.so
sde64 -dmr -- $t/exe3 | grep 42

$CC -shared -o $t/c.so $t/a.o -Wl,-no-relax
$CC -B. -o $t/exe4 $t/b.o $t/c.so -Wl,-no-relax
sde64 -dmr -- $t/exe4 | grep 42
