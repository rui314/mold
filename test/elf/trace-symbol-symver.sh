#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
void foo1() {}
void foo2() {}
void foo3() {}

__asm__(".symver foo1, foo@VER1");
__asm__(".symver foo2, foo@VER2");
__asm__(".symver foo3, foo@@VER3");
EOF

cat <<EOF > $t/b.version
VER1 { local: *; };
VER2 { local: *; };
VER3 { local: *; };
EOF

$CC -B. -o $t/c.so -shared $t/a.o -Wl,--version-script=$t/b.version \
   -Wl,--trace-symbol='foo@VER1' > /dev/null

cat <<EOF | $CC -c -o $t/d.o -xc -
void foo();
__asm__(".symver foo, foo@VER1");
int main() { foo(); }
EOF

$CC -B. -o $t/exe $t/d.o $t/c.so -Wl,--trace-symbol='foo@VER1' > /dev/null
$QEMU $t/exe
