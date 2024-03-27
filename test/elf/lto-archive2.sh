#!/bin/bash
. $(dirname $0)/common.inc

echo 'int main() {}' | $CC -flto=auto -o /dev/null -xc - >& /dev/null || skip

echo | $CC -o $t/a.o -c -flto=auto -xc -

rm -f $t/b.a
ar rc $t/b.a $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe -flto=auto $t/c.o $t/b.a
