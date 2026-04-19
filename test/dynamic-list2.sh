#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo(int x) {}
void bar(int x) {}
EOF

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
void baz(int x) {}
int main() {}
EOF

$CXX -B. -o $t/exe $t/a.o $t/b.o

readelf --dyn-syms $t/exe > $t/log
not grep ' foo' $t/log
not grep ' bar' $t/log

cat <<EOF > $t/dyn
{ foo; extern "C++" { "baz(int)"; }; };
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o -Wl,-dynamic-list=$t/dyn

readelf --dyn-syms $t/exe1 > $t/log1
grep ' foo' $t/log1
not grep ' bar' $t/log1
grep ' _Z3bazi' $t/log1

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,--export-dynamic-symbol-list=$t/dyn

readelf --dyn-syms $t/exe2 > $t/log2
grep ' foo' $t/log2
not grep ' bar' $t/log2
grep ' _Z3bazi' $t/log2
