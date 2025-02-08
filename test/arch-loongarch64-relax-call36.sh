#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.globl foo, bar
.space 0x100000
foo:
  move      $s0,   $ra
  .reloc ., R_LARCH_CALL36, print
  .reloc ., R_LARCH_RELAX
  pcaddu18i $t0,   0
  jirl      $ra,   $t0, 0
  move      $ra,   $s0
  ret
bar:
  .reloc ., R_LARCH_CALL36, print
  .reloc ., R_LARCH_RELAX
  pcaddu18i $t0,   0
  jirl      $zero, $t0, 0
.space 0x100000
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

void foo();
void bar();

void print() {
  printf("foo");
}

int main() {
  foo();
  bar();
  printf("\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o -Wl,--no-relax
$QEMU $t/exe1 | grep foofoo

$OBJDUMP -d $t/exe1 > $t/exe1.objdump
grep -A2 '<foo>:' $t/exe1.objdump | grep -w pcaddu18i
grep -A2 '<bar>:' $t/exe1.objdump | grep -w pcaddu18i

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,--relax
$QEMU $t/exe2 | grep foofoo

$OBJDUMP -d $t/exe2 > $t/exe2.objdump
grep -A2 '<foo>:' $t/exe2.objdump | grep -w bl
grep -A2 '<bar>:' $t/exe2.objdump | grep -w b
