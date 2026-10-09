#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

rm -f $t/b.a
ar rcs $t/b.a $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe1 $t/b.a $t/c.o
nm $t/exe1 > $t/log1
! grep -q _foo $t/log1 || false

$CC --ld-path=$mold -o $t/exe2 $t/b.a $t/c.o -Wl,-u,_foo
nm $t/exe2 > $t/log2
grep -q _foo $t/log2
