#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/libfoo.so -shared -fPIC -Wl,-soname,libfoo.so -xc -
int fn1() { return 42; }
EOF

cat <<EOF | $CC -o $t/libbar.so -shared -fPIC -Wl,-soname,libbar.so -xc -
int fn1();
int fn2() { return fn1(); }
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
int fn2();
int main() { fn2(); }
EOF

$CC -B. -o $t/exe $t/a.o -L$t -Wl,--as-needed -lbar -lfoo
readelf -W --dynamic $t/exe > $t/log2
grep -q libbar $t/log2
not grep -q libfoo $t/log2
