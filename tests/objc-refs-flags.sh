#!/bin/bash
source "$(dirname "$0")"/common.inc

# The Objective-C runtime finds the selector and class references by
# name, and their literal-pointer type and no-dead-strip attribute
# direct the linker alone: a final image makes them plain data, in
# __DATA or, in the shared region or from macOS 14.4 on, __DATA_CONST,
# whatever their input types. (ld-prime keeps those bits where it finds
# the sections in __DATA.) A -r output keeps the input's flags, which
# the next link reads.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__objc_methname,cstring_literals
L_m: .asciz "foo"
.section __DATA,__objc_selrefs,literal_pointers,no_dead_strip
.p2align 3
.quad L_m
.section __DATA,__objc_classrefs,regular,no_dead_strip
.p2align 3
.quad _x
.data
.globl _x
_x:
.quad 0
EOF

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

cp $t/a.o $t/b.o
set_flags $t/b.o __DATA __objc_selrefs 0

sect() {
  otool -l $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1; next }
    f && $1 == "segname" { g = $2 } f && $1 == "flags" { print g, $2; f = 0 }'
}

$CC --ld-path=$mold -dynamiclib -o $t/a.dylib $t/a.o
[ "$(sect $t/a.dylib __objc_selrefs)" = '__DATA 0x00000000' ]
[ "$(sect $t/a.dylib __objc_classrefs)" = '__DATA_CONST 0x00000000' ]

$CC --ld-path=$mold -dynamiclib -o $t/a2.dylib $t/a.o -Wl,-no_data_const
[ "$(sect $t/a2.dylib __objc_classrefs)" = '__DATA 0x00000000' ]

$CC --ld-path=$mold -dynamiclib -o $t/b.dylib $t/b.o
[ "$(sect $t/b.dylib __objc_selrefs)" = '__DATA 0x00000000' ]

$CC --ld-path=$mold -dynamiclib -o $t/a3.dylib $t/a.o -Wl,-install_name,/usr/lib/liba.dylib
[ "$(sect $t/a3.dylib __objc_selrefs)" = '__DATA_CONST 0x00000000' ]

$CC --ld-path=$mold -dynamiclib -o $t/b3.dylib $t/b.o -Wl,-install_name,/usr/lib/libb.dylib
[ "$(sect $t/b3.dylib __objc_selrefs)" = '__DATA_CONST 0x00000000' ]

$mold -r -arch $ARCH -o $t/r.o $t/a.o
[ "$(sect $t/r.o __objc_selrefs)" = '__DATA 0x10000005' ]
[ "$(sect $t/r.o __objc_classrefs)" = '__DATA 0x10000000' ]
