#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.s
.section .tbss,"awT",@nobits
.globl v8, v16, v32, v64, v128
.p2align 4
v8:
  .zero 1
.balign 2
v16:
  .zero 2
.balign 4
v32:
  .zero 4
.balign 8
v64:
  .zero 8
.balign 16
v128:
  .zero 16

.text
.globl set8
set8:
  mrs x1, tpidr_el0
  add x1, x1, #:tprel_hi12:v8, lsl #12
  strb w0, [x1, #:tprel_lo12_nc:v8]
  ret
.globl get8
get8:
  mrs x1, tpidr_el0
  add x1, x1, #:tprel_hi12:v8, lsl #12
  ldrb w0, [x1, #:tprel_lo12_nc:v8]
  ret

.globl set16
set16:
  mrs x1, tpidr_el0
  add x1, x1, #:tprel_hi12:v16, lsl #12
  strh w0, [x1, #:tprel_lo12_nc:v16]
  ret
.globl get16
get16:
  mrs x1, tpidr_el0
  add x1, x1, #:tprel_hi12:v16, lsl #12
  ldrh w0, [x1, #:tprel_lo12_nc:v16]
  ret

.globl set32
set32:
  mrs x1, tpidr_el0
  add x1, x1, #:tprel_hi12:v32, lsl #12
  str w0, [x1, #:tprel_lo12_nc:v32]
  ret
.globl get32
get32:
  mrs x1, tpidr_el0
  add x1, x1, #:tprel_hi12:v32, lsl #12
  ldr w0, [x1, #:tprel_lo12_nc:v32]
  ret

.globl set64
set64:
  mrs x1, tpidr_el0
  add x1, x1, #:tprel_hi12:v64, lsl #12
  str x0, [x1, #:tprel_lo12_nc:v64]
  ret
.globl get64
get64:
  mrs x1, tpidr_el0
  add x1, x1, #:tprel_hi12:v64, lsl #12
  ldr x0, [x1, #:tprel_lo12_nc:v64]
  ret

.globl set128
set128:
  mrs x2, tpidr_el0
  add x2, x2, #:tprel_hi12:v128, lsl #12
  fmov d0, x0
  mov v0.d[1], x1
  str q0, [x2, #:tprel_lo12_nc:v128]
  ret
.globl get128
get128:
  mrs x2, tpidr_el0
  add x2, x2, #:tprel_hi12:v128, lsl #12
  ldr q0, [x2, #:tprel_lo12_nc:v128]
  fmov x0, d0
  mov x1, v0.d[1]
  ret
EOF

cat <<'EOF' > $t/main.c
#include <stdio.h>

void set8(unsigned char);
unsigned char get8(void);
void set16(unsigned short);
unsigned short get16(void);
void set32(unsigned int);
unsigned int get32(void);
void set64(unsigned long);
unsigned long get64(void);
void set128(__int128);
__int128 get128(void);

int main() {
  set8(0x12);
  set16(0x1234);
  set32(0x12345678);
  set64(0x123456789abcdef0UL);
  set128(((__int128)0x1111111111111111UL << 64) | 0x2222222222222222UL);
  __int128 v = get128();
  printf("%x %x %x %lx %lx %lx\n", get8(), get16(), get32(), get64(),
         (unsigned long)(v >> 64), (unsigned long)v);
  return 0;
}
EOF

# GNU as cannot assemble the 128-bit load/store form, so use clang for the
# assembly.
clang_args=()
[ "$TRIPLE" = "" ] || clang_args+=(--target=$TRIPLE)
clang "${clang_args[@]}" -c -o $t/a.o $t/a.s
$CC -c -o $t/main.o $t/main.c

# GNU objdump cannot read R_AARCH64_TLSLE_LDST128_TPREL_LO12_NC, so use readelf
# to check that the test covers all five relocation types.
readelf -rW $t/a.o > $t/relocs.txt
for n in 8 16 32 64 128; do
  grep -q "R_AARCH64_TLSLE_LDST${n}_TPREL_LO12_NC" $t/relocs.txt || exit 1
done

$CC -B. -o $t/exe $t/a.o $t/main.o
$QEMU $t/exe | grep '12 1234 12345678 123456789abcdef0 1111111111111111 2222222222222222'
