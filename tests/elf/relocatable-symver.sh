#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
void foo1() {}
void foo2() {}
void foo3() {}

__asm__(".symver foo1, foo@VER1");
__asm__(".symver foo2, foo@VER2");
__asm__(".symver foo3, foo@@VER3");
EOF

./mold -r -o $t/b.o $t/a.o
readelf --symbols $t/b.o > $t/log1
grep -F 'foo@VER1' $t/log1
grep -F 'foo@VER2' $t/log1
grep -F 'foo@@VER3' $t/log1

echo 'VER1 { local: *; }; VER2 { local: *; }; VER3 { local: *; };' > $t/c.ver
$CC -B. -shared -o $t/d.so $t/b.o -Wl,--version-script=$t/c.ver
readelf --dyn-syms $t/d.so > $t/log2
grep -F 'foo@VER1' $t/log2
grep -F 'foo@VER2' $t/log2
grep -F 'foo@@VER3' $t/log2
