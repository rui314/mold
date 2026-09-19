#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
char *hello() { return "Hello"; }
EOF

$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -Wl,-umbrella,Foo
otool -l $t/b.dylib | grep -q 'umbrella Foo'
