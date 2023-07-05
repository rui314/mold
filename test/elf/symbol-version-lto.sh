#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc - -flto
void foo_1() {}
__asm__(".symver foo_1, foo@@VER1");
EOF

echo 'VER1 { foo; };' > $t/b.ver
$CC -B. -shared -o $t/c.so $t/a.o -Wl,--version-script=$t/b.ver -flto
readelf --symbols $t/c.so > $t/log

grep -Fq 'foo@@VER1' $t/log
