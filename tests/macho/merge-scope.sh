#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _foo
.weak_def_can_be_hidden _foo
.p2align 2
_foo:
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl _foo
.weak_definition _foo
.p2align 2
_foo:
  ret
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
objdump --macho --exports-trie $t/exe | grep -q _foo
