#!/usr/bin/env bash
. $(dirname $0)/common.inc

# libbar refers to fn1 in libfoo but does not list libfoo.so in
# DT_NEEDED. As in GNU ld, an output that needs libbar.so then depends
# on libfoo.so as well, so that fn1 can be found at run time.

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
grep libbar $t/log2
grep libfoo $t/log2

cat <<EOF | $CC -o $t/b.o -c -fPIC -xc -
int fn2();
int fn3() { return fn2(); }
EOF

$CC -B. -o $t/libbaz.so -shared $t/b.o -L$t -Wl,--as-needed -lbar -lfoo
readelf -W --dynamic $t/libbaz.so > $t/log3
grep libbar $t/log3
grep libfoo $t/log3
