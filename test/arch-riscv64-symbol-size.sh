#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl get_foo, get_bar
.type get_foo @function
get_foo:
  lui a0, %hi(foo)
  add a0, a0, %lo(foo)
  ret
.size get_foo, .-get_foo

.type get_bar @function
get_bar:
  lui a0, %hi(bar)
  add a0, a0, %lo(bar)
  ret
.size get_bar, .-get_bar
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl foo, bar
foo = 0xf00
bar = 0xf00
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int get_foo();
int get_bar();
int main() { printf("%x %x\n", get_foo(), get_bar()); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o

readelf --syms $t/a.o | grep -E ' 10 FUNC .* get_foo$'
readelf --syms $t/a.o | grep -E ' 10 FUNC .* get_bar$'

readelf --syms $t/exe | grep -E ' 8 FUNC .* get_foo$'
readelf --syms $t/exe | grep -E ' 8 FUNC .* get_bar$'
