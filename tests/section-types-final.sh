#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image has only the section types ld-prime lays out: zero fill
# (S_GB_ZEROFILL is plain zero fill, whatever the file holds), strings
# and literals, initializer and terminator lists, the thread-local
# kinds, and DOF if bare. Pointers, stubs, interposing tuples and init
# offsets of an input section are regular data there, and only a
# regular or coalesced section of pure instructions stays code. A
# __TEXT,__const keeps its type, but a literal pool folded into it is
# regular.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__const
.asciz "abc"
.section __TEXT,__str
.asciz "ab"
.section __DATA,__lp
.p2align 3
.quad _x
.section __DATA,__lazy
.p2align 3
.quad _x
.section __DATA,__stb
.quad 0
.section __DATA,__ip
.p2align 3
.quad _x
.quad _x
.section __DATA,__dof
.quad 0
.section __DATA,__dof2
.quad 0
.section __DATA,__tlvp
.p2align 3
.quad _x
.section __DATA,__io
.long 0
.section __DATA,__code
.quad 0
.section __DATA,__gbz
.globl _gb
.p2align 3
_gb:
.quad 0x1234
.data
.globl _x
_x:
.quad 0
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__literal8,8byte_literals
.quad 0x5678
EOF

cat <<EOF | $CC -o $t/main.o -c -xc -
extern long gb;
int main() { return gb != 0; }
EOF

# Rewrites the flags of sections: arguments are FILE, then SEG SECT
# FLAGS for each section.
set_flags() {
  python3 - "$@" <<'EOF'
import struct, sys
path, args = sys.argv[1], sys.argv[2:]
want = {(args[i], args[i + 1]): int(args[i + 2], 0) for i in range(0, len(args), 3)}
d = bytearray(open(path, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        name = (d[s + 16:s + 32].rstrip(b'\0').decode(), d[s:s + 16].rstrip(b'\0').decode())
        if name in want:
            struct.pack_into('<I', d, s + 64, want[name])
    off += size
open(path, 'wb').write(d)
EOF
}

set_flags $t/a.o __TEXT __const 0x2 __TEXT __str 0x80000002 \
  __DATA __lp 0x5 __DATA __lazy 0x7 __DATA __stb 0x8 __DATA __ip 0xd \
  __DATA __dof 0xf __DATA __dof2 0x0400000f __DATA __tlvp 0x14 \
  __DATA __io 0x16 __DATA __code 0x8000000b __DATA __gbz 0xc

sect() {
  otool -l $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1; next }
    f && $1 == "segname" { g = $2 } f && $1 == "flags" { print g, $2; f = 0 }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/main.o
[ "$(sect $t/exe __const)" = '__TEXT 0x00000002' ]
[ "$(sect $t/exe __str)" = '__TEXT 0x00000002' ]
[ "$(sect $t/exe __lp)" = '__DATA 0x00000000' ]
[ "$(sect $t/exe __lazy)" = '__DATA 0x00000000' ]
[ "$(sect $t/exe __stb)" = '__DATA 0x00000000' ]
[ "$(sect $t/exe __ip)" = '__DATA 0x00000000' ]
[ "$(sect $t/exe __dof)" = '__DATA 0x0000000f' ]
[ "$(sect $t/exe __dof2)" = '__DATA 0x00000000' ]
[ "$(sect $t/exe __tlvp)" = '__DATA 0x00000000' ]
[ "$(sect $t/exe __io)" = '__DATA 0x00000000' ]
[ "$(sect $t/exe __code)" = '__DATA 0x80000400' ]
[ "$(sect $t/exe __gbz)" = '__DATA 0x00000001' ]
$t/exe

$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/a.o $t/main.o
[ "$(sect $t/exe2 __const)" = '__TEXT 0x00000000' ]
