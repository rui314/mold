#!/bin/bash
source "$(dirname "$0")"/common.inc

# Both objects use subsections: without them ld-prime makes _foo, which
# names a whole section, non-weak, and rejects the two as duplicates
# (see whole-section-weak.sh).
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
