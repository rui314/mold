#!/bin/bash
. $(dirname $0)/common.inc

# Only run on x86_64 and i386 for simplicity of assembler syntax
[[ $MACHINE = x86_64 ]] || [[ $MACHINE = i386 ]] || skip

# Define absolute symbol
cat <<EOF | $CC -c -o $t/a.o -xassembler -
.globl foo
foo = 0
EOF

# Use PC-relative relocation to it
cat <<EOF | $CC -fPIC -c -o $t/b.o -xassembler -
.text
.globl bar
bar:
  .long foo - .
EOF

# Try to link into shared library.
# Should succeed with our fix.
$CC -B. -shared -o $t/libtest.so $t/a.o $t/b.o
