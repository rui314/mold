#!/bin/bash
source "$(dirname "$0")"/common.inc

# Under a -segalign below 8, __LINKEDIT starts wherever the segment
# before it ends. ld-prime aligns its tables to file offsets, not to the
# segment's start: the dyld opcodes and the chained fixups start where
# the table before them ends (__LINKEDIT's start, for the first), the
# others on 8 bytes (the code signature on 16).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.section __TEXT,__cstring,cstring_literals
  .asciz "a"
EOF

link() { $mold -arch $ARCH -platform_version macos 13.0 13.0 -syslibroot "$SDK" -lSystem "$@"; }

# Each table's offset, by its load command field.
offsets() {
  otool -l $1 | awk '$1 == "cmd" { c = $2 } $1 == "segname" { s = $2 }
    $1 ~ /off$/ && $2 != 0 { print (c == "LC_SEGMENT_64" ? s : c) "." $1, $2 }'
}
off() { awk -v k=$1 '$1 == k { print $2 }' $t/offs; }

link -segalign 0x1 $t/a.o -o $t/exe1
offsets $t/exe1 > $t/offs
start=$(off __LINKEDIT.fileoff)
[ $((start % 8)) != 0 ]
[ $(off LC_DYLD_CHAINED_FIXUPS.dataoff) = $start ]
for k in LC_DYLD_EXPORTS_TRIE.dataoff LC_FUNCTION_STARTS.dataoff LC_SYMTAB.symoff \
  LC_SYMTAB.stroff; do
  [ $(($(off $k) % 8)) = 0 ]
done
[ $ARCH = x86_64 ] || [ $(($(off LC_CODE_SIGNATURE.dataoff) % 16)) = 0 ]

# A string's length that leaves __LINKEDIT 4 bytes off.
for len in 1 2 3 4 5 6 7 8; do
  cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int main(void) { puts("$(printf "%${len}s" | tr ' ' x)"); return 0; }
EOF
  link -segalign 0x4 -no_fixup_chains $t/b.o -o $t/exe2 2> /dev/null
  otool -l $t/exe2 | awk '$1 ~ /_(off|size)$/ { print $1, $2 }' > $t/offs
  start=$(off rebase_off)
  [ $((start % 8)) = 0 ] || break
done
[ $((start % 8)) != 0 ]
[ $(off bind_off) = $((start + $(off rebase_size))) ]
[ $(off lazy_bind_off) = $(($(off bind_off) + $(off bind_size))) ]
[ $(($(off export_off) % 8)) = 0 ]

$mold -arch $ARCH -static -e _main -segalign 0x1 $t/a.o -o $t/exe3
offsets $t/exe3 > $t/offs
[ $(($(off __LINKEDIT.fileoff) % 8)) != 0 ]
[ $(($(off LC_SYMTAB.symoff) % 8)) = 0 ]
