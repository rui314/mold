#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo = 3;
EOF

rm -f $t/b.a
ar rc $t/b.a $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
int bar = 5;
EOF

rm -f $t/d.a
ar rc $t/d.a $t/c.o

cat <<EOF | $CC -o $t/e.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/e.o -Wl,-force_load,$t/b.a $t/d.a

nm $t/exe > $t/log
grep -q 'D _foo$' $t/log
! grep -q 'D _bar$' $t/log || false
