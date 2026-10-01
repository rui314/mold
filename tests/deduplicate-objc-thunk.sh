#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime folds a Swift function ("_$s...") even where its address is
# taken or it is exported, but not an @objc thunk ("...To"), which an
# Objective-C method list names: that folds only as a C function does,
# when it is hidden and its address is not taken.
[ $ARCH = arm64 ] || skip

obj() {
  cat <<EOF | $CC -o $t/$1.o -c -xassembler -
.text
.globl _\$s1A1fyyF
.p2align 2
_\$s1A1fyyF:
  mov x0, #7
  ret
.globl $2
$3
.p2align 2
$2:
  mov x0, #7
  ret
.globl _use
.p2align 2
_use:
  bl _\$s1A1fyyF
  bl $2
  ret
.section __DATA,__const
.p2align 3
_tab:
  .quad $4
.subsections_via_symbols
EOF
  $CC --ld-path=$mold -shared -o $t/$1.dylib $t/$1.o -Wl,-deduplicate
  nm -n $t/$1.dylib | grep 's1A1' | awk '{ print $1 }' | sort -u | wc -l | tr -d ' '
}

# Address taken: a thunk stays apart, any other Swift function folds.
[ "$(obj a '_$s1A1gyyFTo' '' '_$s1A1gyyFTo')" = 2 ]
[ "$(obj b '_$s1A1gyyF' '' '_$s1A1gyyF')" = 1 ]
[ "$(obj c '_$s1A1gyyFTo' '.private_extern _$s1A1gyyFTo' '_$s1A1gyyFTo')" = 2 ]
# Not taken: an exported thunk stays apart, a hidden one folds.
[ "$(obj d '_$s1A1gyyFTo' '' '_use')" = 2 ]
[ "$(obj e '_$s1A1gyyFTo' '.private_extern _$s1A1gyyFTo' '_use')" = 1 ]
