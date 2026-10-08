#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Mapping symbols ($a, $t and $d) are STT_NOTYPE, including the ones mold
# synthesizes for PLT entries and thunks.

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.syntax unified
.globl foo
.type foo, %function
.type bar, %function
.thumb
.thumb_func
foo:
  b.w bar
.arm
bar:
  bx lr
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
void foo();
int main() {
  foo();
  printf("Hello\n");
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep Hello

readelf -sW $t/exe > $t/log
grep -F 'bar$thunk' $t/log
grep -F 'puts$plt' $t/log
not grep -E 'FUNC +LOCAL +DEFAULT +[0-9]+ \$[atd]$' $t/log
