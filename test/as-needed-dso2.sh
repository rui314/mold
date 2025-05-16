#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/libfoo.so -shared -fPIC -Wl,-soname,libfoo.so -xc -
int fn1() { return 42; }
EOF

cat <<EOF | $CC -o $t/libbar.so -shared -fPIC -Wl,-soname,libbar.so -xc - -L$t -lfoo
int fn1();
int fn2() { return fn1(); }
EOF

cat <<EOF | $CC -o $t/libbaz.so -shared -fPIC -Wl,-soname,libbaz.so -xc - -L$t -lbar
int fn2();
int fn3() { return fn2(); }
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
int fn3();
int main() { fn3(); }
EOF

$CC -B. -o $t/exe $t/a.o -L$t -Wl,--as-needed -lbaz -lbar
readelf -W --dynamic $t/exe > $t/log2

not grep libfoo $t/log2
not grep libbar $t/log2
grep libbaz $t/log2
