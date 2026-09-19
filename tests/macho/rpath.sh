#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int three() { return 3; }
EOF2

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o \
  -install_name @rpath/libfoo.dylib

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int three();
int main() { printf("%d\n", three()); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/b.o -L$t -lfoo -Wl,-rpath,$PWD/$t
otool -l $t/exe | grep -A2 LC_RPATH | grep -q path
$t/exe | grep '^3$'
