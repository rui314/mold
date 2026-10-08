#!/bin/bash
source "$(dirname "$0")"/common.inc

# Without .subsections_via_symbols, ld-prime cuts a code section at its
# labels for __unwind_info, and gives a label inside a function -
# within the length of the function's compact unwind record or FDE -
# an entry of encoding 0 like any label without a record of its own.
# The code past it then can't be unwound: a backtrace from _f's call,
# which follows the label _inner, stops at it. mold leaves such a label
# no entry, so the record covers the whole function. ld-prime fails
# this test.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <dlfcn.h>
#include <stdio.h>
#include <unwind.h>
void f(void);
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
int main() { f(); }
EOF

if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _f, _inner
.p2align 2
_f:
  stp x29, x30, [sp, #-16]!
  mov x29, sp
_inner:
  bl _trace
  ldp x29, x30, [sp], #16
  ret
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _f
.long 20
.long 0x04000000
.quad 0
.quad 0
EOF
else
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _f, _inner
_f:
  pushq %rbp
  movq %rsp, %rbp
_inner:
  callq _trace
  popq %rbp
  retq
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _f
.long 11
.long 0x01000000
.quad 0
.quad 0
EOF
fi

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
[ "$($RUN $t/exe)" = 'trace inner main ' ]
objdump --unwind-info $t/exe > $t/unwind
inner=$(nm $t/exe | awk '$3 == "_inner" { print $1 }' | sed 's/^0*1/0x/')
not grep -q "function offset=$inner," $t/unwind
