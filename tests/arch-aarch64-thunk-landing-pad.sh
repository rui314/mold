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
# .text.high2 has to be placed next to .text.high1.
cat <<EOF | $CC -c -o $t/c.o -xassembler -
  .section .text.high1, "ax"
high1:
  mov w0, #42
  ret

  .section .text.high2, "ax"
high2:
  mov w0, #42
  ret

  .section .low, "ax"
  .globl _start
_start:
  bl high1
  mov w19, w0
  bl high2
  add w0, w0, w19
  cmp w0, #84
  cset x0, ne
  mov x8, #93 // exit
  svc #0
EOF

$CC -B. -static -nostdlib -o $t/exe3 $t/c.o -Wl,-z,force-bti,--icf=all \
  -Wl,--section-start=.low=0x10000000,--section-start=.text=0x20000000
$QEMU $t/exe3
