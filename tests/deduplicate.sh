#!/usr/bin/env bash
. $(dirname $0)/common.inc

# ld-prime folds identical functions of __TEXT,__text whose addresses no
# one can compare: those marked .weak_def_can_be_hidden, which the link
# hides (C++ inline functions with unnamed_addr), even if their address
# is taken, and any other unexported one only if nothing but branches
# refers to it. An exported function, weak or not, keeps an address of
# its own. ld-prime deduplicates at -O1 and up or with -deduplicate;
# mold always unless -no_deduplicate.
if [ $ARCH = arm64 ]; then
  body() { echo "mov w0, #$1"; echo ret; }
  call() { echo "bl $1"; }
  prologue='stp x29, x30, [sp, #-16]!'
  epilogue='ldp x29, x30, [sp], #16'
else
  body() { echo "movl \$$1, %eax"; echo ret; }
  call() { echo "call $1"; }
  prologue='pushq %rbp'
  epilogue='popq %rbp'
fi

{
  echo '.subsections_via_symbols'
  echo '.text'
  # Auto-hidden and exported weak functions, each pair's address taken
  echo '.globl _ah1, _ah2, _wd1, _wd2'
  echo '.weak_def_can_be_hidden _ah1, _ah2'
  echo '.weak_definition _wd1, _wd2'
  echo '_ah1:'; body 1
  echo '_ah2:'; body 1
  echo '_wd1:'; body 2
  echo '_wd2:'; body 2
  # Local functions, one pair only called and one pair's address taken
  echo '_call1:'; body 3
  echo '_call2:'; body 3
  echo '_ptr1:'; body 4
  echo '_ptr2:'; body 4
  # Private externals only called
  echo '.globl _pe1, _pe2'
  echo '.private_extern _pe1, _pe2'
  echo '_pe1:'; body 5
  echo '_pe2:'; body 5
  echo '.globl _call_all'
  echo '_call_all:'
  echo "$prologue"
  call _call1; call _call2; call _pe1; call _pe2
  echo "$epilogue"
  echo 'ret'
  echo '.data'
  echo '.globl _ptrs'
  echo '.p2align 3'
  echo '_ptrs:'
  echo '.quad _ah1, _ah2, _wd1, _wd2, _ptr1, _ptr2'
} > $t/a.s
$CC -o $t/a.o -c $t/a.s

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
extern void *ptrs[6];
void call_all(void);
int main() {
  call_all();
  printf("%d %d %d\n", ptrs[0] == ptrs[1], ptrs[2] == ptrs[3], ptrs[4] == ptrs[5]);
}
EOF

# Two symbols name one address
same_addr() {
  a=$(grep " $2\$" $1 | cut -d' ' -f1)
  b=$(grep " $3\$" $1 | cut -d' ' -f1)
  [ -n "$a" ] && [ "$a" = "$b" ]
}

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-deduplicate
$t/exe | grep '^1 0 0$'
nm $t/exe > $t/nm
same_addr $t/nm _call1 _call2
same_addr $t/nm _pe1 _pe2

# -no_deduplicate keeps them apart
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -Wl,-no_deduplicate
$t/exe2 | grep '^0 0 0$'
nm $t/exe2 > $t/nm2
not same_addr $t/nm2 _call1 _call2

# The last of -deduplicate and -no_deduplicate wins.
$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/b.o -Wl,-deduplicate -Wl,-no_deduplicate
$t/exe3 | grep '^0 0 0$'
$CC --ld-path=$mold -o $t/exe4 $t/a.o $t/b.o -Wl,-no_deduplicate -Wl,-deduplicate
$t/exe4 | grep '^1 0 0$'
