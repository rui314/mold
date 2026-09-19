#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int provided() { return 9; }
EOF2

$CC --ld-path=$mold -shared -o $t/libinner.dylib $t/a.o \
  -install_name $PWD/$t/libinner.dylib

cat <<EOF2 | $CC -o $t/b.o -c -xc -
int wrapper() { return 1; }
EOF2

# libouter re-exports libinner: clients of libouter see "provided".
$CC --ld-path=$mold -shared -o $t/libouter.dylib $t/b.o \
  -install_name $PWD/$t/libouter.dylib \
  -Wl,-reexport_library,$t/libinner.dylib
otool -l $t/libouter.dylib | grep -q LC_REEXPORT_DYLIB

cat <<EOF2 | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int provided();
int wrapper();
int main() { printf("%d\n", provided() + wrapper()); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/c.o $t/libouter.dylib
$t/exe | grep '^10$'
