#!/bin/bash
source "$(dirname "$0")"/common.inc

# Both objects use subsections: without them a section is one atom
# whose first symbol ld64 never treats as weak, and two such _foo
# definitions are a duplicate-symbol error (ld-prime rejects them).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _foo
.weak_def_can_be_hidden _foo
.p2align 2
_foo:
  ret
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl _foo
.weak_definition _foo
.p2align 2
_foo:
  ret
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
objdump --macho --exports-trie $t/exe | grep _foo
