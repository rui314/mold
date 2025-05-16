#!/bin/bash
. $(dirname $0)/common.inc

[ "$(uname)" = FreeBSD ] && skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
void f1() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
void f1();
void f2() { f1(); }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -fPIC
void f1();
void f3() { f1(); }
EOF

$CC -B. -o $t/libfoo.so -shared $t/a.o
$CC -B. -o $t/libbar.so -shared $t/b.o -L$t -lfoo
$CC -B. -o $t/libbaz.so -shared $t/c.o


cat <<EOF | $CC -o $t/d.o -c -xc - -fPIC
void f2();
void f3();
int main() { f2(); f3(); }
EOF

$CC -B. -o $t/exe $t/d.o -L$t -lbar -lbaz
