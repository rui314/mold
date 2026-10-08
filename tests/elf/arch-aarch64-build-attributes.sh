#!/usr/bin/env bash
. $(dirname $0)/common.inc

# GCC 16 records -mbranch-protection in AArch64 build attributes instead
# of .note.gnu.property. Skip if the compiler is older.
echo 'void foo() {}' | $CC -mbranch-protection=bti -c -o $t/a.o -xc - || skip
readelf --sections $t/a.o | grep -F .ARM.attributes || skip

# _start calls foo with an indirect branch, which must land on a BTI
# landing pad if the output is BTI-enabled.
cat <<EOF | $CC -O2 -mbranch-protection=standard -c -o $t/b.o -xc -
int foo();
int (*fp)() = foo;

void _start() {
  register long x0 __asm__("x0") = fp() == 42 ? 0 : 1;
  register long x8 __asm__("x8") = 93; // exit
  __asm__ volatile("svc 0" :: "r"(x0), "r"(x8));
}
EOF

cat <<EOF | $CC -O2 -mbranch-protection=bti -c -o $t/c.o -xc -
int foo() { return 42; }
EOF

cat <<EOF | $CC -O2 -mbranch-protection=none -c -o $t/d.o -xc -
int foo() { return 42; }
EOF

./mold -static -o $t/exe1 $t/b.o $t/c.o
readelf -n $t/exe1 | grep 'AArch64 feature: BTI$'
$QEMU $t/exe1

./mold -static -o $t/exe2 $t/b.o $t/d.o
readelf -n $t/exe2 | not grep 'AArch64 feature'
$QEMU $t/exe2

./mold -r -o $t/e.o $t/b.o $t/c.o
readelf -n $t/e.o | grep 'AArch64 feature: BTI$'
