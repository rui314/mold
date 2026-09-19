#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o

cat <<EOF | $CC -o $t/b.o -c -xc -
void bar() {}
EOF

$CC --ld-path=$mold -shared -o $t/libbar.dylib $t/b.o -Wl,-reexport_library,$t/libfoo.dylib

objdump --macho --dylibs-used $t/libbar.dylib | grep -q 'libfoo.*reexport'

cat <<EOF | $CC -o $t/c.o -c -xc -
void baz() {}
EOF

$CC --ld-path=$mold -shared -o $t/libbaz.dylib $t/c.o -Wl,-reexport_library,$t/libbar.dylib

objdump --macho --dylibs-used $t/libbaz.dylib | grep -q 'libbar.*reexport'

cat <<EOF | $CC -o $t/d.o -c -xc -
void foo();
void bar();
void baz();

int main() {
  foo();
  bar();
  baz();
}
EOF

$CC -o $t/exe $t/d.o -L$t -lbaz
