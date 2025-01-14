#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -O2 -march=rv64g
void g() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -O2 -march=rv64g
void g();
void f() { g(); }
int main() { f(0); }
EOF

$CC -B. -march=rv64g -o $t/exe1 $t/a.o $t/b.o
$QEMU $t/exe1
$OBJDUMP -d $t/exe1 | grep -Eq '\bj\b.*<g>'


cat <<EOF | $CC -o $t/c.o -c -xc - -O2 -march=rv64gc
void g() {}
EOF

cat <<EOF | $CC -o $t/d.o -c -xc - -O2 -march=rv64gc
void g();
void f() { g(); }
int main() { f(0); }
EOF

$CC -B. -march=rv64g -o $t/exe2 $t/c.o $t/d.o
$QEMU $t/exe2
$OBJDUMP -d $t/exe2 | grep -Eq '\bj\b.*<g>'
