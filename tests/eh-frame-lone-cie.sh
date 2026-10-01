#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime carries a CIE no FDE points at into __eh_frame like any
# other atom, in its place among the records and with a GOT slot for
# its personality to point at, in a final image unless -dead_strip
# drops it, and in a -r output. A CIE whose FDEs all go - here _g's,
# which has a compact unwind record - goes with them.
if [ $ARCH = arm64 ]; then
  ret=ret ra=30 sp='0x0c, 31, 0' got='@GOT - .'
else
  ret=retq ra=16 sp='0x0c, 7, 8' got=@GOTPCREL
fi
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main, _g
.p2align 2
_main:
  $ret
_g:
  $ret
.section __TEXT,__eh_frame
EH_frame0:
.long 20
.long 0
.byte 1, 0x7a, 0x52, 0, 1, 0x78, $ra, 1, 0x10, $sp, 0, 0, 0, 0
.long 24
.long 28
.quad _main - .
.quad 1
.long 0
EH_frame1:
.long 24
.long 0
.byte 1
.asciz "zPR"
.byte 1, 0x78, $ra, 6, 0x9b
.long ___gcc_personality_v0$got
.byte 0x10, $sp, 0, 0
EH_frame2:
.long 20
.long 0
.byte 1, 0x7a, 0x52, 0, 1, 0x78, $ra, 1, 0x10, $sp, 0, 0, 0, 0
.long 24
.long 28
.quad _g - .
.quad 1
.long 0
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _g
.long 1
.long 0x02000000
.quad 0
.quad 0
.subsections_via_symbols
EOF

# Prints the kinds of an __eh_frame's records, and the personality
# pointer of the zPR CIE as an address.
eh_frame() {
  python3 - $1 <<'EOF2'
import struct, subprocess, sys
out = subprocess.run(['otool', '-l', sys.argv[1]], capture_output=True, text=True).stdout.splitlines()
for i, l in enumerate(out):
    if l.strip() == 'sectname __eh_frame':
        addr = int(out[i + 2].split()[1], 16)
        size = int(out[i + 3].split()[1], 16); off = int(out[i + 4].split()[1])
d = open(sys.argv[1], 'rb').read()[off:off + size]
pos = 0
while pos < len(d):
    length, id = struct.unpack_from('<II', d, pos)
    print('FDE' if id else 'CIE', end=' ')
    if d[pos + 9:pos + 13] == b'zPR\0':
        cell = pos + 18
        print(hex(addr + cell + struct.unpack_from('<i', d, cell)[0]), end=' ')
    pos += 4 + length
print()
EOF2
}

$CC --ld-path=$mold -o $t/exe $t/a.o
eh_frame $t/exe > $t/records
slot=$(dyld_info -fixups $t/exe | awk '$NF ~ /___gcc_personality_v0$/ { print tolower($3) }')
[ "$(cat $t/records)" = "CIE FDE CIE $slot " ]

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-dead_strip
eh_frame $t/exe2 > $t/records2
[ "$(cat $t/records2)" = 'CIE FDE ' ]

$mold -r -arch $ARCH -o $t/b.o $t/a.o
eh_frame $t/b.o | sed 's/0x[0-9a-f]* //' > $t/records3
[ "$(cat $t/records3)" = 'CIE FDE CIE CIE FDE ' ]
