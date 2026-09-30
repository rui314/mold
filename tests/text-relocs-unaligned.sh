#!/bin/bash
source "$(dirname "$0")"/common.inc

# Unaligned pointers are diagnosed after the text relocations: ld-prime
# lists those as it applies relocations, each atom's in address order
# where it encodes rebase opcodes and from the last to the first
# otherwise. An unaligned pointer of an arm64 chain then fails the link
# in place of the text relocations (a text relocation is in no chain);
# an x86-64 image gives chains up for classic dyld info first. The
# classic info's warnings of unaligned pointers come last, only if the
# link has not failed.
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

dir=$(cd $t && pwd -P)
not $CC --ld-path=$mold -o $t/exe1 $t/a.o 2> $t/log1
grep -v '^[+]\|^clang' $t/log1 > $t/list1
if [ $ARCH = arm64 ]; then
  cat <<EOF > $t/expected1
Illegal text-relocations:
  text-relocation in '_tp'+0xC ($dir/a.o) to '_main'
  text-relocation in '_tp'+0x4 ($dir/a.o) to '_main'
EOF
  tail -1 $t/list1 | grep -qF "pointer not aligned in '_dp'+0x4 ($dir/a.o)"
  not grep -q 'Found illegal text-relocations' $t/list1
else
  cat <<EOF > $t/expected1
Illegal text-relocations:
  text-relocation in '_tp'+0x4 ($dir/a.o) to '_main'
  text-relocation in '_tp'+0xC ($dir/a.o) to '_main'
EOF
  head -1 $t/list1 | grep -q 'warning: disabling chained fixups because of unaligned pointers'
  tail -1 $t/list1 | grep -q 'Found illegal text-relocations'
  not grep -q 'pointer not aligned' $t/list1
fi
grep -E '^(Illegal|  text-relocation)' $t/list1 > $t/relocs1
diff $t/expected1 $t/relocs1

not $CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_fixup_chains 2> $t/log2
grep -E '^(Illegal|  text-relocation)' $t/log2 > $t/relocs2
diff - $t/relocs2 <<EOF
Illegal text-relocations:
  text-relocation in '_tp'+0x4 ($dir/a.o) to '_main'
  text-relocation in '_tp'+0xC ($dir/a.o) to '_main'
EOF
grep -q 'Found illegal text-relocations' $t/log2
not grep -q 'pointer not aligned' $t/log2

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
grep -qF "warning: pointer not aligned in '_dp'+0x4 ($dir/b.o)" $t/log3
grep -qF "warning: pointer not aligned in '_dp'+0x1C ($dir/b.o)" $t/log3
