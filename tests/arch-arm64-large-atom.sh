#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# ld-prime puts its branch islands in clusters at most 124 MiB of code
# apart, each island a b to the next, so a code atom of that size is one
# no branch can cross that way; it warns about each. (Our thunks reach
# 4 GiB, so we can still link a branch across one; ld-prime fails.)
cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.globl _big
.p2align 2
_big:
  .space 0x7c00000
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o 2> $t/log
grep -qF "warning: atom '_big' ($(cd $t && pwd -P)/a.o) is larger than the max code size between branch island clusters, this may lead to unreachable branches" $t/log
[ "$(grep -c 'branch island clusters' $t/log)" = 1 ]
rm -f $t/a.o $t/exe
