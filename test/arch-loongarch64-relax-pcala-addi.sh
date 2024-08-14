#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.globl get_sym1, get_sym2, get_sym3
get_sym1:
  la.pcrel $a0, sym1
  ret
get_sym2:
  la.pcrel $a0, sym2
  ret
get_sym3:
  la.pcrel $a0, sym3
  ret
EOF

cat <<'EOF' | $CC -o $t/b.o -c -xassembler -
.globl sym1, sym2, sym3
sym1:
  li.d $a0, 1
  ret
.space 1024 * 1024
sym2:
  li.d $a0, 2
  ret
.space 1024 * 1024
sym3:
  li.d $a0, 3
  ret
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

int (*get_sym1())();
int (*get_sym2())();
int (*get_sym3())();

int main() {
  printf("%d %d %d\n", get_sym1()(), get_sym2()(), get_sym3()());
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -Wl,--no-relax
$QEMU $t/exe1 | grep -q '^1 2 3$'

$OBJDUMP -d $t/exe1 > $t/exe1.objdump
grep -A1 '<get_sym1>:' $t/exe1.objdump | grep -q pcalau12i
grep -A1 '<get_sym2>:' $t/exe1.objdump | grep -q pcalau12i
grep -A1 '<get_sym3>:' $t/exe1.objdump | grep -q pcalau12i

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o -Wl,--relax
$QEMU $t/exe2 | grep -q '^1 2 3$'

$OBJDUMP -d $t/exe2 > $t/exe2.objdump
grep -A1 '<get_sym1>:' $t/exe2.objdump | grep -q pcaddi
grep -A1 '<get_sym2>:' $t/exe2.objdump | grep -q pcaddi
grep -A1 '<get_sym3>:' $t/exe2.objdump | grep -q pcalau12i
