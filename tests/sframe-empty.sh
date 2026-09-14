#!/usr/bin/env bash
. $(dirname $0)/common.inc

# GNU assembler 2.45 emits an empty .sframe section for an input file that
# needs no unwind info (e.g. glibc's Scrt1.o). The linker must skip such a
# section instead of reading a header past its end.
# https://github.com/rui314/mold/issues/1646

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section .sframe,"a"
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() { return 0; }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe
