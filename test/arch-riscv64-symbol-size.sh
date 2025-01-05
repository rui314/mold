#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl get_foo
.type get_foo @function
get_foo:
  lui a0, %hi(foo)
  add a0, a0, %lo(foo)
  ret
.size get_foo, .-get_foo
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl foo, bar, baz
foo = 0xf00
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int get_foo();
int main() { printf("%x\n", get_foo()); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o

readelf --syms $t/a.o | grep -Eq ' 10 FUNC .* get_foo$'
readelf --syms $t/exe | grep -Eq ' 8 FUNC .* get_foo$'
