#!/bin/bash
source "$(dirname "$0")"/common.inc

mkdir -p $t/foo/bar

cat <<EOF | $CC -shared -o $t/foo/bar/libbaz.dylib -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo();
void bar() { foo(); }
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o -nodefaultlibs \
  -L/foo/bar -isysroot $t -lbaz
