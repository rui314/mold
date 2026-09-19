#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int foo() { return 0; }
EOF2

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -Wl,-application_extension
otool -hv $t/libfoo.dylib | grep -q APP_EXTENSION_SAFE
