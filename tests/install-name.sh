#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o -Wl,-install_name,foobar
otool -l $t/b.dylib | grep 'name foobar'

# -dylib_install_name and -dylinker_install_name are other spellings of
# it, as in ld64.
$CC --ld-path=$mold -shared -o $t/c.dylib $t/a.o -Wl,-dylib_install_name,foobar2
otool -l $t/c.dylib | grep -q 'name foobar2 '
$CC --ld-path=$mold -shared -o $t/d.dylib $t/a.o -Wl,-dylinker_install_name,foobar3
otool -l $t/d.dylib | grep -q 'name foobar3 '
