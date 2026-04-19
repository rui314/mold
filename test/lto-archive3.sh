#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -flto=auto || skip

cat <<EOF | $CC -o $t/a.o -c -xc - -flto
void foo() {}
EOF

rm -f $t/b.a
ar rc $t/b.a $t/a.o
ar rc $t/c.a $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC -B. -o $t/exe -flto $t/c.o $t/b.a $t/c.a
