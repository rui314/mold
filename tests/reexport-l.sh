#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o

cat <<EOF | $CC -o $t/b.o -c -xc -
void bar() {}
EOF

$CC --ld-path=$mold -shared -o $t/libbar.dylib $t/b.o -L$t -Wl,-reexport-lfoo

objdump --macho --dylibs-used $t/libbar.dylib | grep 'libfoo.*reexport'

cat <<EOF | $CC -o $t/c.o -c -xc -
void baz() {}
EOF

$CC --ld-path=$mold -shared -o $t/libbaz.dylib $t/c.o -L$t -Wl,-reexport-lbar

objdump --macho --dylibs-used $t/libbaz.dylib | grep 'libbar.*reexport'

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

# -reexport-l looks for a dylib only, skipping an archive of the name
# in an earlier directory: an archive cannot be re-exported.
mkdir -p $t/ar
rm -f $t/ar/libfoo.a
ar rcs $t/ar/libfoo.a $t/a.o
$CC --ld-path=$mold -shared -o $t/libqux.dylib $t/c.o -L$t/ar -L$t -Wl,-reexport-lfoo
objdump --macho --dylibs-used $t/libqux.dylib | grep -q 'libfoo.*reexport'
not $CC --ld-path=$mold -shared -o $t/libqux.dylib $t/c.o -L$t/ar -Wl,-reexport-lfoo 2> $t/log
grep -q "library 'foo' not found" $t/log
