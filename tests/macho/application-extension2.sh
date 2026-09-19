#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

$CC --ld-path=$mold -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
void bar() {}
EOF

$CC --ld-path=$mold -shared -o $t/d.so $t/b.so $t/c.o \
  -Wl,-application_extension >& $t/log

grep -q 'not safe for use in application extensions' $t/log
