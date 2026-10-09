#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime packs the LC_DYLD_CHAINED_FIXUPS payload tightly: the 28-byte
# header padded to 8, the starts_in_image table, each segment's starts
# record on an 8-byte boundary with a size counting just its 22 bytes of
# fields and a u16 per page, then the import table aligned only as its
# entries need (4 bytes, 8 for 64-bit addends), the names, and padding
# to 8. Here __PAGEZERO, __TEXT, __DATA and __LINKEDIT make four
# segments, so the starts table ends 4 bytes past an 8-byte boundary.
[ $ARCH = arm64 ] && page=16384 || page=4096

header() {
  dyld_info -fixup_chain_header $1 | awk '$1 == "'$2'" { print $2; exit }'
}

datasize() {
  otool -l $1 | grep -A3 LC_DYLD_CHAINED_FIXUPS | awk '$1 == "datasize" { print $2 }'
}

echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

# A two-page record: 26 bytes at 0x38, so the imports start at 0x54.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.data
.p2align 3
.globl _a
_a: .quad _free
.space $page
.quad _a
EOF

$CC --ld-path=$mold -o $t/exe1 $t/main.o $t/a.o -mmacosx-version-min=13.0
$RUN $t/exe1
[ $(header $t/exe1 size) = 0x0000001A ]
[ $(header $t/exe1 imports_offset) = 0x00000054 ]
[ $(datasize $t/exe1) = 96 ]

# 64-bit addends align the imports to 8.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.p2align 3
.globl _a
_a: .quad _free + 0x100000000
.space $page
.quad _a
EOF

$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/b.o -mmacosx-version-min=13.0
$RUN $t/exe2
[ $(header $t/exe2 imports_format) = 0x00000003 ]
[ $(header $t/exe2 imports_offset) = 0x00000058 ]
[ $(datasize $t/exe2) = 112 ]

# Nothing to fix up: the empty import table follows the starts table.
echo 'int x = 1;' | $CC -o $t/c.o -c -xc -
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/c.o -mmacosx-version-min=13.0
$RUN $t/exe3
[ $(header $t/exe3 seg_count) = 0x00000004 ]
[ $(header $t/exe3 imports_offset) = 0x00000034 ]
[ $(datasize $t/exe3) = 56 ]
