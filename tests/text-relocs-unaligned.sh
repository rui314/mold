#!/bin/bash
source "$(dirname "$0")"/common.inc

# A pointer in a read-only segment is a text relocation, which fails
# the link; an unaligned pointer elsewhere fails an arm64 chain, and
# makes an x86-64 image give chains up for classic dyld info.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
.section __TEXT,__const
.globl _tp
.p2align 3
_tp:
  .long 0
  .quad _main
  .quad _main
.data
.globl _dp
.p2align 3
_dp:
  .long 0
  .quad _main
EOF

dir=$t
cat <<EOF > $t/expected
  text-relocation in '_tp'+0x4 ($dir/a.o) to '_main'
  text-relocation in '_tp'+0xC ($dir/a.o) to '_main'
EOF
not $CC --ld-path=$mold -o $t/exe1 $t/a.o 2> $t/log1
if [ $ARCH = arm64 ]; then
  grep -q "pointer not aligned.*'_dp'+0x4 ($dir/a.o)" $t/log1
else
  grep -q 'warning: disabling chained fixups because of unaligned pointers' $t/log1
  grep -E '^  text-relocation' $t/log1 | sort | diff $t/expected -
  grep -q 'Found illegal text-relocations' $t/log1
fi

not $CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_fixup_chains 2> $t/log2
grep -E '^  text-relocation' $t/log2 | sort | diff $t/expected -
grep -q 'Found illegal text-relocations' $t/log2

# Without text relocations, the warnings come.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _main
_main:
  ret
.data
.globl _dp
.p2align 3
_dp:
  .long 0
  .quad _main
  .long 0
  .quad _main
  .long 0
  .quad _main
EOF
$CC --ld-path=$mold -o $t/exe3 $t/b.o -Wl,-no_fixup_chains 2> $t/log3
grep -q "warning: pointer not aligned.*'_dp'+0x4 ($dir/b.o)" $t/log3
grep -q "warning: pointer not aligned.*'_dp'+0x1C ($dir/b.o)" $t/log3
