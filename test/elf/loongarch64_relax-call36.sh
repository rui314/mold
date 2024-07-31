#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.globl foo, bar
foo:
  move      $s0,   $ra
  .reloc ., R_LARCH_CALL36, foo2
  .reloc ., R_LARCH_RELAX
  pcaddu18i $t0,   0
  jirl      $ra,   $t0, 0
  move      $ra,   $s0
  ret
bar:
  .reloc ., R_LARCH_CALL36, bar2
  .reloc ., R_LARCH_RELAX
  pcaddu18i $t0,   0
  jirl      $zero, $t0, 0
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
void foo();
void bar();
void foo2() { printf("foo"); }
void bar2() { printf("bar"); }

int main() {
  foo();
  bar();
  printf("\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o -Wl,--no-relax
$QEMU $t/exe1 | grep -q foobar

$OBJDUMP -d $t/exe1 > $t/exe1.objdump
grep -A2 '<foo>:' $t/exe1.objdump | grep -wq pcaddu18i
grep -A2 '<bar>:' $t/exe1.objdump | grep -wq pcaddu18i

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,--relax
$QEMU $t/exe2 | grep -q foobar

$OBJDUMP -d $t/exe2 > $t/exe2.objdump
grep -A2 '<foo>:' $t/exe2.objdump | grep -wq bl
grep -A2 '<bar>:' $t/exe2.objdump | grep -wq b
