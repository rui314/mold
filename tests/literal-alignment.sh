#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# A fixed-size literal is aligned to its size, whatever its section
# header says. Compilers emit a 16-byte constant whose type is only
# 8-aligned into __literal16 with p2align 3 (rustc's (1, 0) splitn
# initializer), then load it with ldr q, whose PAGEOFF12 immediate is
# scaled by 16 and cannot address an 8-mod-16 slot. ld64 gives every
# __literal4/8/16 atom Alignment(2/3/4) with no modulus, so the linker
# is what makes the load sound. Placing such a literal at its input
# offset modulo 16 (as every other atom is placed) silently loads the
# neighboring slot: rustc libtest parsed --list as "ist".
#
# a.o starts __literal16 at 8 mod 16, after an 8-byte __const. b.o has
# a 16-aligned copy of the same bytes and loads it through a private
# label: literal merge keeps one copy, and whichever survives must be
# 16-aligned for both loads. c.o is the 8-byte case: a __literal8 at 4
# mod 8, loaded with ldr d.
cat <<EOF2 | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__const
.p2align 3
.quad 0x1111111111111111

.section __TEXT,__literal16,16byte_literals
.p2align 3
.globl _lCPI
_lCPI:
  .quad 1, 0
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__literal16,16byte_literals
.p2align 4
LCPI0_0:
  .quad 1, 0

.section __TEXT,__text,regular,pure_instructions
.globl _load_local
.p2align 2
_load_local:
  adrp x8, LCPI0_0@PAGE
  ldr q0, [x8, LCPI0_0@PAGEOFF]
  mov x0, v0.d[0]
  mov x1, v0.d[1]
  ret
EOF2

cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.section __TEXT,__const
.p2align 2
.long 0x22222222

.section __TEXT,__literal8,8byte_literals
.p2align 2
.globl _l8
_l8:
  .long 3, 4
EOF2

cat <<EOF2 | $CC -o $t/load.o -c -xassembler -
.globl _load_cpi
.p2align 2
_load_cpi:
  adrp x8, _lCPI@PAGE
  ldr q0, [x8, _lCPI@PAGEOFF]
  mov x0, v0.d[0]
  mov x1, v0.d[1]
  ret

.globl _load_l8
.p2align 2
_load_l8:
  adrp x8, _l8@PAGE
  ldr d0, [x8, _l8@PAGEOFF]
  fmov x0, d0
  ret
EOF2

cat <<EOF2 | $CC -o $t/main.o -c -xc -
#include <stdint.h>
#include <stdio.h>

typedef struct { unsigned long lo, hi; } Pair;
Pair load_cpi(void);
Pair load_local(void);
unsigned long load_l8(void);
extern unsigned long lCPI[2];
extern unsigned long l8;

int main(void) {
  Pair a = load_cpi();
  Pair b = load_local();
  printf("%lu %lu %lu %lu %lx %d %d\n", a.lo, a.hi, b.lo, b.hi, load_l8(),
         (int)((uintptr_t)lCPI % 16), (int)((uintptr_t)&l8 % 8));
  return 0;
}
EOF2

# The under-aligned copy comes first and survives the merge.
$CC --ld-path=$mold -o $t/exe1 $t/main.o $t/load.o $t/a.o $t/b.o $t/c.o
$t/exe1 | grep '^1 0 1 0 400000003 0 0$'

# The aligned copy comes first.
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/load.o $t/b.o $t/a.o $t/c.o
$t/exe2 | grep '^1 0 1 0 400000003 0 0$'

# Outside a literal section the input offset is honored, and a 16-byte
# load of an 8-mod-16 address is an error rather than a load of the
# wrong slot (ld-prime drops the low bits silently).
cat <<EOF2 | $CC -o $t/d.o -c -xassembler -
.section __TEXT,__const
.p2align 3
.quad 9
.globl _v
_v:
  .quad 1, 0
EOF2

cat <<EOF2 | $CC -o $t/loadv.o -c -xassembler -
.globl _load_v
.p2align 2
_load_v:
  adrp x8, _v@PAGE
  ldr q0, [x8, _v@PAGEOFF]
  ret
EOF2

cat <<EOF2 | $CC -o $t/mainv.o -c -xc -
void load_v(void);
int main(void) { load_v(); }
EOF2

not $CC --ld-path=$mold -o $t/exe3 $t/mainv.o $t/loadv.o $t/d.o 2> $t/err
grep -qF "fixup error (kind=arm64_lo12) at '_load_v'+0x4 from loadv.o, target '_v' not 16-byte aligned" $t/err
