#!/usr/bin/env bash
. $(dirname $0)/common.inc

echo 'int foo(void) { return 42; }' | $CC -fPIC -c -o $t/a.o -xc -
printf 'INPUT(\013%s)\n' "$PWD/$t/a.o" > $t/script
./mold -shared -o $t/a.so $t/script
readelf -sW $t/a.so | grep -E ' foo$'
