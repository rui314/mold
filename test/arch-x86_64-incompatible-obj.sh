#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -m32 || skip

cat <<EOF | $CC -c -o $t/a.o -m64 -xc -
int main() {}
EOF

cat <<EOF | $CC -c -o $t/b.o -m32 -xc -
EOF

not $CC -B. -o /dev/null $t/a.o $t/b.o >& $t/log
grep "$t/b.o: incompatible file type: x86_64 is expected but got i386" $t/log
