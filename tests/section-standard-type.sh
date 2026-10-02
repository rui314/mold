#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime takes an input section of a standard name for that standard
# section only if it has the type the name implies - but for the
# Objective-C runtime's sections (and __got), which it knows by name
# alone. Only such a section gets the standard flags or moves to
# __DATA_CONST: a __mod_init_func assembled without its type stays where
# data of its name goes. A __literal8 merges into __const by its name
# whatever its type (ld-prime keeps one without its type apart), but a
# section a -rename_section names __literal8 or __StaticInit does not.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__objc_methname
.asciz "foo"
.section __DATA,__objc_protolist
.p2align 3
.quad _x
.section __DATA,__mod_init_func
.p2align 3
.quad 0
.section __TEXT,__literal8
.quad 0x1234
.section __TEXT,__bar
.quad 0x5678
.section __TEXT,__baz,regular,pure_instructions
.long 0
.section __DATA,__const
.quad 1
.data
.globl _x
_x: .quad 0
EOF
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

# Rewrites the flags of the sections named SEG,SECT in FILE.
set_flags() {
  python3 - "$@" <<'EOF'
import struct, sys
path, seg, sect, flags = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4], 0)
d = bytearray(open(path, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        if d[s:s + 16].rstrip(b'\0') == sect.encode() and d[s + 16:s + 32].rstrip(b'\0') == seg.encode():
            struct.pack_into('<I', d, s + 64, flags)
    off += size
open(path, 'wb').write(d)
EOF
}
# A coalesced __DATA,__const, which isn't the standard one either.
set_flags $t/a.o __DATA __const 0xb
set_flags $t/a.o __TEXT __literal8 0

sect() {
  otool -l $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1; next }
    f && $1 == "segname" { g = $2 } f && $1 == "flags" { print g, $2; f = 0 }'
}

otool -lv $t/a.o > $t/a.lc
grep -A10 'sectname __objc_methname' $t/a.lc | grep -q 'type S_REGULAR'
grep -A10 'sectname __objc_protolist' $t/a.lc | grep -q 'type S_REGULAR'
grep -A10 'sectname __mod_init_func' $t/a.lc | grep -q 'type S_REGULAR'

$mold -arch $ARCH -r -o $t/r.o $t/a.o
[ "$(sect $t/r.o __objc_methname)" = '__TEXT 0x00000002' ]
[ "$(sect $t/r.o __objc_protolist)" = '__DATA 0x0000000b' ]
[ "$(sect $t/r.o __mod_init_func)" = '__DATA 0x00000000' ]
[ "$(sect $t/r.o __literal8)" = '__TEXT 0x00000000' ]

$CC --ld-path=$mold -o $t/exe $t/a.o $t/main.o \
  -Wl,-rename_section,__TEXT,__bar,__TEXT,__literal8 \
  -Wl,-rename_section,__TEXT,__baz,__TEXT,__StaticInit
[ "$(sect $t/exe __objc_methname)" = '__TEXT 0x00000002' ]
[ "$(sect $t/exe __objc_protolist)" = '__DATA_CONST 0x00000000' ]
[ "$(sect $t/exe __mod_init_func)" = '__DATA 0x00000000' ]
[ "$(sect $t/exe __const | sort | tr '\n' ' ')" = '__DATA 0x00000000 __TEXT 0x00000000 ' ]
[ "$(sect $t/exe __literal8)" = '__TEXT 0x00000000' ]
[ "$(sect $t/exe __StaticInit)" = '__TEXT 0x80000400' ]
$t/exe
