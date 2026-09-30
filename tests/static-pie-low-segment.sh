#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -static -pie image's local relocations count their addresses from
# the first segment (on x86-64 the first writable one). A pointer in a
# segment pinned below that, as XNU pins __HIB below the kernel, gets a
# negative address in the signed 32-bit r_address; one too far away for
# it fails the link (ld-prime).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
.data
.p2align 3
_d: .quad _main
.section __HIB,__data
.p2align 3
_h: .quad _main
EOF

$mold -arch $ARCH -static -pie -e _main -pagezero_size 0 -image_base 0xffffff8000200000 \
  -segaddr __HIB 0xffffff8000100000 $t/a.o -o $t/exe
if [ $ARCH = arm64 ]; then seg=__TEXT; else seg=__DATA; fi
base=$(otool -l $t/exe | awk -v s=$seg '$1 == "segname" && $2 == s { getline; print $2; exit }')
h=0x$(nm $t/exe | awk '$3 == "_h" { print $1 }')
off=$(otool -l $t/exe | awk '$1 == "locreloff" { print $2 }')
n=$(otool -l $t/exe | awk '$1 == "nlocrel" { print $2 }')
od -A n -t x4 -j $off -N $((n * 8)) $t/exe | tr ' ' '\n' > $t/words
grep -qx "$(printf '%08x' $(((h - base) & 0xffffffff)))" $t/words

not $mold -arch $ARCH -static -pie -e _main -pagezero_size 0 -image_base 0xffffff8000200000 \
  -segaddr __HIB 0xffffff0000000000 $t/a.o -o $t/exe2 2> $t/log2
grep -q "atom address cannot fit in a fixup at '_h' (.*/a.o)+0" $t/log2
