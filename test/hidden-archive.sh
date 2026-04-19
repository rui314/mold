#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -fPIC -xc -
void foo() {}
EOF

rm -f $t/b.a
ar rcs $t/b.a $t/a.o

cat <<EOF | $CC -shared -o $t/c.so -fPIC -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/d.o -fPIC -c -xc -
__attribute__((visibility("hidden"))) void foo();
int main() { foo(); }
EOF

$CC -B. -o $t/exe $t/d.o $t/c.so $t/b.a
$QEMU $t/exe
