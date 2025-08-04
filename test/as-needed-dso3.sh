#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/libfoo.so -shared -fPIC -Wl,-soname,libfoo.so -xc -
int fn1() { return 42; }
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

$CC -B. -o $t/exe1 $t/a.o -L$t -Wl,--no-as-needed -lfoo -Wl,--no-as-needed
readelf -W --dynamic $t/exe1 > $t/log1
grep -F 'Shared library: [libfoo.so]' $t/log1

$CC -B. -o $t/exe2 $t/a.o -L$t -lfoo -Wl,--no-as-needed -lfoo -Wl,--no-as-needed
readelf -W --dynamic $t/exe2 > $t/log2
grep -F 'Shared library: [libfoo.so]' $t/log2

$CC -B. -o $t/exe3 $t/a.o -L$t -Wl,--no-as-needed -lfoo -Wl,--no-as-needed -lfoo
readelf -W --dynamic $t/exe3 > $t/log3
grep -F 'Shared library: [libfoo.so]' $t/log3
