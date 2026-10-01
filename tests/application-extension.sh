#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int foo() { return 0; }
EOF2

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -Wl,-application_extension
otool -hv $t/libfoo.dylib | grep APP_EXTENSION_SAFE

# ld-prime marks dylibs only, not an executable or a bundle.
echo 'int main() { return 0; }' | $CC -o $t/b.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-application_extension
otool -hv $t/exe > $t/log
not grep -q APP_EXTENSION_SAFE $t/log
$CC --ld-path=$mold -bundle -o $t/c.bundle $t/a.o -Wl,-application_extension
otool -hv $t/c.bundle > $t/log
not grep -q APP_EXTENSION_SAFE $t/log
