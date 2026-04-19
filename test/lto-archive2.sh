#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -flto=auto || skip

echo | $CC -o $t/a.o -c -flto=auto -xc -

rm -f $t/b.a
ar rc $t/b.a $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe -flto=auto $t/c.o $t/b.a
