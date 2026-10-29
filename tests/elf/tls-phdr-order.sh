#!/usr/bin/env bash
. $(dirname $0)/common.inc

# uClibc's ld.so stops scanning the executable's program headers at
# PT_TLS, so PT_DYNAMIC has to come before it, as GNU ld puts it.
cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
#include <stdio.h>
_Thread_local int x = 42;
int main() { printf("%d\n", x); }
EOF

$CC -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep '^42$'
readelf -lW $t/exe | grep -E '^ +(DYNAMIC|TLS) ' > $t/log
head -1 $t/log | grep DYNAMIC
grep TLS $t/log
