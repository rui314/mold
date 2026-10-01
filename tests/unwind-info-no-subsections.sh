#!/bin/bash
source "$(dirname "$0")"/common.inc

# Without .subsections_via_symbols a section is one atom to dead-strip
# and to order, but ld-prime still cuts it into atoms at its labels for
# __unwind_info, and each one without a compact unwind record of its
# own gets an entry of encoding 0 ("no unwind info"), so that it does
# not fall under the record of the function before it. Here _g lies
# between the records of _f and _h: unwound by _f's frame rules, its
# caller would be wrong, so the unwinder stops at it instead.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <dlfcn.h>
#include <stdio.h>
#include <unwind.h>
void f(void), g(void);
static _Unwind_Reason_Code print_frame(struct _Unwind_Context *ctx, void *arg) {
  int *n = arg;
  Dl_info info;
  if (dladdr((char *)_Unwind_GetIP(ctx) - 1, &info) && info.dli_sname)
    printf("%s ", info.dli_sname);
  else
    printf("? ");
  return ++*n < 10 ? _URC_NO_REASON : _URC_END_OF_STACK;
}
void trace(void) { int n = 0; _Unwind_Backtrace(print_frame, &n); }
int main(int argc, char **argv) { if (argc > 1) g(); else f(); }
EOF

if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _f, _g, _h
.p2align 2
_f:
  stp x29, x30, [sp, #-16]!
  mov x29, sp
  bl _trace
  ldp x29, x30, [sp], #16
  ret
_g:
  sub sp, sp, #16
  str x30, [sp, #8]
  bl _trace
  ldr x30, [sp, #8]
  add sp, sp, #16
  ret
_h:
  ret
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _h
.long 4
.long 0x02000000
.quad 0
.quad 0
.quad _f
.long 20
.long 0x04000000
.quad 0
.quad 0
EOF
else
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _f, _g, _h
_f:
  pushq %rbp
  movq %rsp, %rbp
  callq _trace
  popq %rbp
  retq
_g:
  subq \$8, %rsp
  callq _trace
  addq \$8, %rsp
  retq
_h:
  retq
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _h
.long 1
.long 0x02010000
.quad 0
.quad 0
.quad _f
.long 7
.long 0x01000000
.quad 0
.quad 0
EOF
fi

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
[ "$($t/exe)" = 'trace f main ' ]
[ "$($t/exe g)" = 'trace ' ]

# Each label starts an atom, an alternate entry point's too; of two
# labels at one place the first names an empty atom, as does one at the
# section's end. A record anywhere in an atom is the atom's, so the
# start of _a, whose record is past it, gets no entry. Encoding 0 is
# used often enough to be a common encoding.
if [ $ARCH = arm64 ]; then ret=ret; n=4; else ret=retq; n=1; fi
rec() { printf '.quad %s\n.long %s\n.long %s\n.quad 0\n.quad 0\n' $1 $n $2; }
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  $ret
_a:
  $ret
  $ret
_b:
  $ret
.alt_entry _c
_c:
  $ret
_d:
_e:
  $ret
_end:
.section __LD,__compact_unwind,regular,debug
.p2align 3
$(rec _main 0x02001000)
$(rec _a+$n 0x02002000)
$(rec _e 0x02003000)
EOF
$CC --ld-path=$mold -o $t/exe2 $t/c.o
objdump --unwind-info $t/exe2 > $t/unwind
main=$(nm $t/exe2 | awk '$3 == "_main" { print $1 }')
sed -En 's/.*function offset=0x([0-9a-f]+), encoding\[[0-9]+\]=(0x[0-9a-f]+)$/\1 \2/p' $t/unwind |
  while read off enc; do echo $(((0x$off + 0x100000000 - 0x$main) / n)) $enc; done > $t/entries
cat > $t/expected <<EOF
0 0x02001000
2 0x02002000
3 0x00000000
4 0x00000000
5 0x00000000
5 0x02003000
6 0x00000000
EOF
diff $t/expected $t/entries
grep -A1 'Common encodings: (count = 1)' $t/unwind | grep -q 'encoding\[0\]: 0x00000000'
