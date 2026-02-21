#!/bin/bash
. $(dirname $0)/common.inc

# This regression reproduces a mixed LTO + regular-object COMDAT case.
# The regular object has a non-COMDAT section referring to a symbol in a
# COMDAT group with signature "mix". An LTO object contributes the same
# COMDAT key. If IR COMDAT ownership is allowed to override reachable regular
# object ownership, the regular COMDAT member can be discarded incorrectly.

[ $MACHINE = $(uname -m) ] || skip

CLANG="${TEST_CLANG:-clang}"
$CLANG --version >& /dev/null || skip
echo 'int main() {}' | $CLANG -B. -flto -o /dev/null -xc - >& /dev/null || skip

cat <<'EOF' > $t/a.ll
$mix = comdat any
@mix_lto = global i32 1, comdat($mix)
EOF

cat <<'EOF' > $t/b.s
.globl keep_me
.section .text.keep_me,"axG",@progbits,mix,comdat
keep_me:
  ret

.text
.globl main
main:
  call keep_me
  xor %eax, %eax
  ret
EOF

$CLANG -S -emit-llvm -flto -o $t/a.bc $t/a.ll
$CC -o $t/b.o -c -x assembler $t/b.s
$CLANG -B. -o $t/exe -flto $t/a.bc $t/b.o

$QEMU $t/exe
