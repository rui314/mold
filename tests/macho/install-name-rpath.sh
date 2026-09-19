#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

mkdir -p $t/x/y/z

$CC --ld-path=$mold -shared -o $t/x/y/z/libfoo.dylib $t/a.o -Wl,-install_name,@rpath/y/z/libfoo.dylib

cat <<EOF | $CC -o $t/b.o -c -xc -
void bar() {}
EOF

$CC --ld-path=$mold -shared -o $t/libbar.dylib $t/b.o -Wl,-reexport_library,$t/x/y/z/libfoo.dylib -Wl,-rpath,$t/x

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

$CC --ld-path=$mold -o $t/exe $t/d.o -L$t -lbaz
