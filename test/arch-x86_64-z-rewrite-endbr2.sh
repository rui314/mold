#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -fcf-protection || skip
[ "$QEMU" == '' ] || skip

# Check if Intel SDE CPU emulator is available
command -v sde64 >& /dev/null || skip

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl hello2
.type hello2, @function
.text
  int3
hello2:
  endbr64
  jmp hello

.section .init_array,"aw",@init_array
.p2align 3
.quad .text+1
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -O -fcf-protection
#include <stdio.h>
void hello() { printf("Hello "); }
void world() { printf("world\n"); }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -O -fcf-protection
void world();
int main() { world(); }
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o
sde64 -cet -cet-endbr-exe -cet_abort -- $t/exe1 | grep -q 'Hello world'

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o -Wl,-z,rewrite-endbr
sde64 -cet -cet-endbr-exe -cet_abort -- $t/exe2 | grep -q 'Hello world'

$OBJDUMP -d $t/exe2 > $t/log2
grep -A1 '<hello2>:' $t/log2 | grep -q endbr64
grep -A1 '<world>:' $t/log2 | grep -q nop
