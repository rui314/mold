#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fcommon -xc -c -o $t/a.o - -Wa,--elf-stt-common=yes 2> /dev/null || skip
int foo;
int bar;
int baz = 42;
EOF

cat <<EOF | $CC -fcommon -xc -c -o $t/b.o - -Wa,--elf-stt-common=yes
#include <stdio.h>

int foo;
int bar = 5;
int baz;

int main() {
  printf("%d %d %d\n", foo, bar, baz);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--fatal-warnings
$QEMU $t/exe | grep '0 5 42'

readelf --sections $t/exe > $t/log
grep '.common .*NOBITS' $t/log
