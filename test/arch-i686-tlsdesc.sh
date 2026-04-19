#!/bin/bash
. $(dirname $0)/common.inc

supports_tlsdesc || skip

cat <<'EOF' | $GCC -c -o $t/a.o -xassembler -
.globl  get_foo
.type   get_foo, @function
get_foo:
  pushl   %ebx
  call    __x86.get_pc_thunk.bx
  addl    $_GLOBAL_OFFSET_TABLE_, %ebx
  subl    $8, %esp
  leal    foo@TLSDESC(%ebx), %ebx
  movl    %ebx, %eax
  call    *foo@TLSCALL(%eax)
  movl    %gs:(%eax), %eax
  addl    $8, %esp
  popl    %ebx
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
$QEMU $t/exe1 | grep 42

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,-no-relax
$QEMU $t/exe2 | grep 42

$CC -B. -shared -o $t/c.so $t/a.o
$CC -B. -o $t/exe3 $t/b.o $t/c.so
$QEMU $t/exe3 | grep 42

$CC -B. -shared -o $t/c.so $t/a.o -Wl,-no-relax
$CC -B. -o $t/exe4 $t/b.o $t/c.so -Wl,-no-relax
$QEMU $t/exe4 | grep 42
