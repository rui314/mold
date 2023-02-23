#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -

void foo1() {}
void foo2() {}

__asm__(".symver foo1, foo@VER1");
__asm__(".symver foo2, foo@@VER2");
EOF

echo 'VER1 { global: foo; }; VER2 { global: foo; };' > $t/b.ver
$CC -B. -shared -o $t/b.so $t/a.o -Wl,--version-script=$t/b.ver

cat <<EOF | $CC -o $t/c.o -c -xc -
void foo();
__asm__(".symver foo, foo@VER1");
int main() { foo(); }
EOF

cat <<EOF | $CC -o $t/d.o -c -xc -
void foo();
__asm__(".symver foo, foo@VER2");
int main() { foo(); }
EOF

cat <<EOF | $CC -o $t/e.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC -B. -o $t/exe1 $t/c.o $t/b.so
$CC -B. -o $t/exe2 $t/d.o $t/b.so
$CC -B. -o $t/exe3 $t/e.o $t/b.so

$QEMU $t/exe1
$QEMU $t/exe2
$QEMU $t/exe3
