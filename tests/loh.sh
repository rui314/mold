#!/bin/bash
source "$(dirname "$0")"/common.inc

# A compiler can't know how far a symbol will land, so it materializes
# addresses with adrp and leaves linker optimization hints naming each
# sequence. The hints are applied as ld64 does: near the target,
# adrp+add becomes adr and a load a literal load; a GOT load of a local
# symbol, relaxed to adrp+add, becomes adr, and of a dylib's symbol a
# literal load of its slot; far away, an add folds into the load or
# store after it; and of two adrp of one page the second goes. ld-prime
# never applies hints, so it fails this test; ld-classic passes it
# where it can read the SDK.
[ $ARCH = arm64 ] || skip

cat <<EOF | $CC -o $t/ext.o -c -xc -
long ext[2] = {5, 6};
EOF
$CC --ld-path=$mold -o $t/libext.dylib -shared $t/ext.o

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.p2align 2
.globl _add, _ldr, _add_ldr, _add_ldrb, _add_str, _got, _got_ext, _got_ldr
.globl _got_ldr_ext, _got_str, _got_str_ext, _adrp_adrp, _far_add_ldr
.globl _far_add_str, _far_got_ldr, _far_got_str
_add:
L1: adrp x0, _v@PAGE
L2: add x0, x0, _v@PAGEOFF
  ret
.loh AdrpAdd L1, L2
_ldr:
L3: adrp x8, _v@PAGE
L4: ldr x0, [x8, _v@PAGEOFF]
  ret
.loh AdrpLdr L3, L4
_add_ldr:
L5: adrp x8, _v@PAGE
L6: add x8, x8, _v@PAGEOFF
L7: ldr x0, [x8]
  ret
.loh AdrpAddLdr L5, L6, L7
_add_ldrb:
L8: adrp x8, _v@PAGE
L9: add x8, x8, _v@PAGEOFF
L10: ldrb w0, [x8]
  ret
.loh AdrpAddLdr L8, L9, L10
_add_str:
L11: adrp x8, _s@PAGE
L12: add x8, x8, _s@PAGEOFF
L13: str x0, [x8]
  ret
.loh AdrpAddStr L11, L12, L13
_got:
L14: adrp x0, _g@GOTPAGE
L15: ldr x0, [x0, _g@GOTPAGEOFF]
  ret
.loh AdrpLdrGot L14, L15
_got_ext:
L16: adrp x0, _ext@GOTPAGE
L17: ldr x0, [x0, _ext@GOTPAGEOFF]
  ret
.loh AdrpLdrGot L16, L17
_got_ldr:
L18: adrp x8, _g@GOTPAGE
L19: ldr x8, [x8, _g@GOTPAGEOFF]
L20: ldr x0, [x8]
  ret
.loh AdrpLdrGotLdr L18, L19, L20
_got_ldr_ext:
L21: adrp x8, _ext@GOTPAGE
L22: ldr x8, [x8, _ext@GOTPAGEOFF]
L23: ldr x0, [x8]
  ret
.loh AdrpLdrGotLdr L21, L22, L23
_got_str:
L24: adrp x8, _g@GOTPAGE
L25: ldr x8, [x8, _g@GOTPAGEOFF]
L26: str x0, [x8]
  ret
.loh AdrpLdrGotStr L24, L25, L26
_got_str_ext:
L27: adrp x8, _ext@GOTPAGE
L28: ldr x8, [x8, _ext@GOTPAGEOFF]
L29: str x0, [x8]
  ret
.loh AdrpLdrGotStr L27, L28, L29
_adrp_adrp:
L30: adrp x8, _v@PAGE
  ldr x0, [x8, _v@PAGEOFF]
L31: adrp x8, _w@PAGE
  ldr x1, [x8, _w@PAGEOFF]
  add x0, x0, x1
  ret
.loh AdrpAdrp L30, L31
_far_add_ldr:
L32: adrp x8, _far@PAGE
L33: add x8, x8, _far@PAGEOFF
L34: ldr x0, [x8]
  ret
.loh AdrpAddLdr L32, L33, L34
_far_add_str:
L35: adrp x8, _far@PAGE
L36: add x8, x8, _far@PAGEOFF
L37: str x0, [x8]
  ret
.loh AdrpAddStr L35, L36, L37
_far_got_ldr:
L38: adrp x8, _far2@GOTPAGE
L39: ldr x8, [x8, _far2@GOTPAGEOFF]
L40: ldr x0, [x8]
  ret
.loh AdrpLdrGotLdr L38, L39, L40
_far_got_str:
L41: adrp x8, _far2@GOTPAGE
L42: ldr x8, [x8, _far2@GOTPAGEOFF]
L43: str x0, [x8]
  ret
.loh AdrpLdrGotStr L41, L42, L43

.zerofill __DATA,__bss,_pad,0x200000,3
.globl _far, _far2
.zerofill __DATA,__bss,_far,8,3
.zerofill __DATA,__bss,_far2,8,3
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
long v = 42, w = 1, g = 3, s = -1;
extern long ext[2], far, far2;
long *add(void), *got(void), *got_ext(void);
long ldr(void), add_ldr(void), add_ldrb(void), got_ldr(void), got_ldr_ext(void);
long adrp_adrp(void), far_add_ldr(void), far_got_ldr(void);
void add_str(long), got_str(long), got_str_ext(long), far_add_str(long), far_got_str(long);

int main() {
  if (*add() != 42 || ldr() != 42 || add_ldr() != 42 || add_ldrb() != 42)
    return 1;
  if (got() != &g || got_ext() != ext || got_ldr() != 3 || got_ldr_ext() != 5)
    return 2;
  add_str(7);
  got_str(8);
  got_str_ext(9);
  if (s != 7 || g != 8 || ext[0] != 9 || adrp_adrp() != 43)
    return 3;
  far_add_str(10);
  far_got_str(11);
  if (far_add_ldr() != 10 || far_got_ldr() != 11 || far != 10 || far2 != 11)
    return 4;
  return 0;
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/libext.dylib
$t/exe
objdump -d --no-show-raw-insn $t/exe > $t/dis

# A function's instructions, with a literal load written ldr=.
insns() {
  sed -n "/<_$1>:/,/ret$/p" $t/dis |
    awk -F'\t' 'NR > 1 { print $2 ($2 == "ldr" && $3 !~ /\[/ ? "=" : "") }' | tr '\n' ' '
}
[ "$(insns add)" = 'adr nop ret ' ]
[ "$(insns ldr)" = 'nop ldr= ret ' ]
[ "$(insns add_ldr)" = 'nop nop ldr= ret ' ]
[ "$(insns add_ldrb)" = 'adr nop ldrb ret ' ]
[ "$(insns add_str)" = 'adr nop str ret ' ]
[ "$(insns got)" = 'adr nop ret ' ]
[ "$(insns got_ext)" = 'nop ldr= ret ' ]
[ "$(insns got_ldr)" = 'nop nop ldr= ret ' ]
[ "$(insns got_ldr_ext)" = 'nop ldr= ldr ret ' ]
[ "$(insns got_str)" = 'adr nop str ret ' ]
[ "$(insns got_str_ext)" = 'nop ldr= str ret ' ]
[ "$(insns adrp_adrp)" = 'adrp ldr nop ldr add ret ' ]
[ "$(insns far_add_ldr)" = 'adrp nop ldr ret ' ]
[ "$(insns far_add_str)" = 'adrp nop str ret ' ]
[ "$(insns far_got_ldr)" = 'adrp nop ldr ret ' ]
[ "$(insns far_got_str)" = 'adrp nop str ret ' ]

# -ignore_optimization_hints keeps the compiler's sequences.
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o $t/libext.dylib -Wl,-ignore_optimization_hints
$t/exe2
objdump -d --no-show-raw-insn $t/exe2 > $t/dis
[ "$(insns add)" = 'adrp add ret ' ]

# So does a dylib bound for the dyld shared cache, unless it opts out.
$CC --ld-path=$mold -o $t/libloh.dylib -shared $t/a.o $t/b.o $t/ext.o \
  -Wl,-install_name,/usr/lib/libloh.dylib
objdump -d --no-show-raw-insn $t/libloh.dylib > $t/dis
[ "$(insns add)" = 'adrp add ret ' ]
$CC --ld-path=$mold -o $t/libloh.dylib -shared $t/a.o $t/b.o $t/ext.o \
  -Wl,-install_name,/usr/lib/libloh.dylib,-not_for_dyld_shared_cache
objdump -d --no-show-raw-insn $t/libloh.dylib > $t/dis
[ "$(insns add)" = 'adr nop ret ' ]
