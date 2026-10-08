#!/usr/bin/env bash
. $(dirname $0)/common.inc

test_cflags -mbranch-protection=bti || skip

# A static function that is only called directly doesn't begin with a BTI
# landing pad, so a range extension thunk has to jump to it via a landing
# pad created by the linker.
cat <<EOF > $t/a.c
__attribute__((section(".high"), noinline)) static int high() { return 42; }

__attribute__((section(".low"))) void _start() {
  register long x0 __asm__("x0") = high() == 42 ? 0 : 1;
  register long x8 __asm__("x8") = 93; // exit
  __asm__ volatile("svc 0" :: "r"(x0), "r"(x8));
}
EOF

$CC -mbranch-protection=bti -O2 -c -o $t/a.o $t/a.c
$OBJDUMP -d $t/a.o | grep -A1 '<high>:' | not grep -E 'bti\s+c'

$CC -B. -static -nostdlib -o $t/exe1 $t/a.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x20000000
readelf -n $t/exe1 | grep 'AArch64 feature: BTI'
$OBJDUMP -d -j .high $t/exe1 | grep -A1 -E 'bti\s+c' | grep -E 'b\s+20000000 <high>'
$QEMU $t/exe1

$CC -mbranch-protection=none -O2 -c -o $t/b.o $t/a.c
$CC -B. -static -nostdlib -o $t/exe2 $t/b.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x20000000
$OBJDUMP -d -j .high $t/exe2 | not grep -E 'bti\s+c'
$QEMU $t/exe2

# .text.high2 is folded into .text.high1 by ICF, so the landing pad for
# high2 has to be placed next to .text.high1. -fno-ipa-icf keeps the
# compiler from folding them first.
cat <<EOF | $CC -mbranch-protection=bti -O2 -fno-ipa-icf -c -o $t/c.o -xc -
__attribute__((section(".text.high1"), noinline)) static int high1() { return 42; }
__attribute__((section(".text.high2"), noinline)) static int high2() { return 42; }

__attribute__((section(".low"))) void _start() {
  register long x0 __asm__("x0") = high1() + high2() == 84 ? 0 : 1;
  register long x8 __asm__("x8") = 93; // exit
  __asm__ volatile("svc 0" :: "r"(x0), "r"(x8));
}
EOF

$CC -B. -static -nostdlib -o $t/exe3 $t/c.o -Wl,--icf=all \
  -Wl,--section-start=.low=0x10000000,--section-start=.text=0x20000000
readelf -n $t/exe3 | grep 'AArch64 feature: BTI'
$QEMU $t/exe3

# Without -ffunction-sections, the compiler refers to f2 and f3 from
# another section by .text plus nonzero offsets, as f1 comes first. The
# linker gives each such branch its own symbol, and their landing pads in
# the same thunk must not be mixed up.
cat <<EOF | $CC -mbranch-protection=bti -O2 -c -o $t/d.o -xc -
__attribute__((noinline)) static int f1() { return 1; }
__attribute__((noinline)) static int f2() { return 2; }
__attribute__((noinline)) static int f3() { return 3; }

int g() { return f1(); }

__attribute__((section(".low"))) void _start() {
  register long x0 __asm__("x0") = f2() * 10 + f3() == 23 ? 0 : 1;
  register long x8 __asm__("x8") = 93; // exit
  __asm__ volatile("svc 0" :: "r"(x0), "r"(x8));
}
EOF

$CC -B. -static -nostdlib -o $t/exe4 $t/d.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.text=0x20000000
readelf -n $t/exe4 | grep 'AArch64 feature: BTI'
$QEMU $t/exe4
