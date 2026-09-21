#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A GOT-relative relocation against a symbol defined in a DSO can only be
# resolved by copying the symbol into the executable (or, for a function,
# by giving it a canonical PLT entry). mold used to resolve it silently as
# if the symbol were at address zero.
# https://github.com/rui314/mold/issues/1668

case $MACHINE in
x86_64)
  cat <<EOF > $t/a.s
  .globl get_foo, get_bar
get_foo:
  lea _GLOBAL_OFFSET_TABLE_(%rip), %rax
  movabs \$foo@GOTOFF, %rdx
  mov (%rax, %rdx), %eax
  ret
get_bar:
  lea _GLOBAL_OFFSET_TABLE_(%rip), %rax
  movabs \$bar@GOTOFF, %rdx
  add %rdx, %rax
  ret
EOF
  ;;
i686)
  cat <<EOF > $t/a.s
  .globl get_foo, get_bar
get_foo:
  call 1f
1:
  pop %ecx
  addl \$_GLOBAL_OFFSET_TABLE_+[.-1b], %ecx
  mov foo@GOTOFF(%ecx), %eax
  ret
get_bar:
  call 1f
1:
  pop %eax
  addl \$_GLOBAL_OFFSET_TABLE_+[.-1b], %eax
  lea bar@GOTOFF(%eax), %eax
  ret
EOF
  ;;
arm | armeb)
  cat <<EOF > $t/a.s
  .globl get_foo, get_bar
  .type get_foo, %function
  .type get_bar, %function
get_foo:
  ldr r0, 1f
  ldr r1, 2f
3:
  add r0, pc, r0
  ldr r0, [r0, r1]
  bx lr
1: .word _GLOBAL_OFFSET_TABLE_ - (3b + 8)
2: .word foo(GOTOFF)
get_bar:
  ldr r0, 1f
  ldr r1, 2f
3:
  add r0, pc, r0
  add r0, r0, r1
  bx lr
1: .word _GLOBAL_OFFSET_TABLE_ - (3b + 8)
2: .word bar(GOTOFF)
EOF
  ;;
s390x)
  cat <<EOF > $t/a.s
  .globl get_foo, get_bar
get_foo:
  larl %r1, _GLOBAL_OFFSET_TABLE_
  larl %r2, 1f
  ag %r1, 0(%r2)
  lgf %r2, 0(%r1)
  br %r14
1: .quad foo@GOTOFF
get_bar:
  larl %r1, _GLOBAL_OFFSET_TABLE_
  larl %r2, 1f
  lg %r2, 0(%r2)
  agr %r2, %r1
  br %r14
1: .quad bar@GOTOFF
EOF
  ;;
sh4 | sh4aeb)
  cat <<EOF > $t/a.s
  .globl get_foo, get_bar
get_foo:
  mov.l 1f, r1
  mova 1f, r0
  add r0, r1
  mov.l 2f, r0
  add r1, r0
  rts
  mov.l @r0, r0
  .align 2
1: .long _GLOBAL_OFFSET_TABLE_
2: .long foo@GOTOFF
get_bar:
  mov.l 1f, r1
  mova 1f, r0
  add r0, r1
  mov.l 2f, r0
  rts
  add r1, r0
  .align 2
1: .long _GLOBAL_OFFSET_TABLE_
2: .long bar@GOTOFF
EOF
  ;;
*)
  skip
  ;;
esac

$CC -c -o $t/a.o $t/a.s

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
#include <stdio.h>
int get_foo();
void *get_bar();
void *bar();
int main() { printf("%d %d\n", get_foo(), get_bar() == bar()); }
EOF

cat <<EOF | $CC -fPIC -shared -o $t/c.so -xc -
int foo = 42;
void *bar() { return bar; }
EOF

$CC -B. -pie -o $t/exe1 $t/a.o $t/b.o $t/c.so
$QEMU $t/exe1 | grep '^42 1$'

$CC -B. -no-pie -o $t/exe2 $t/a.o $t/b.o $t/c.so
$QEMU $t/exe2 | grep '^42 1$'

not $CC -B. -shared -o $t/d.so $t/a.o $t/c.so |& grep 'recompile with -fPIC'
