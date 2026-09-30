#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 warned when an app extension linked a dylib not built with
# -application_extension (no MH_APP_EXTENSION_SAFE); ld-prime says
# nothing, and the output is still marked safe.
cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

$CC --ld-path=$mold -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
void bar() {}
EOF

$CC --ld-path=$mold -shared -o $t/d.so $t/b.so $t/c.o \
  -Wl,-application_extension >& $t/log

not grep -q 'application extensions' $t/log
otool -hv $t/d.so | grep -q APP_EXTENSION_SAFE
