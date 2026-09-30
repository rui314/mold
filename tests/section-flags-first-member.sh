#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime gives an output section the flags of its first member: code
# after data in a section doesn't make it code, nor data after code
# data. Those flags are the ones its table of standard sections holds
# for the member's name when the member has the table's type (a
# regular __TEXT,__const or __DATA,__data an assembler marked as code
# is data, a regular __stub_helper code), and the member's own
# otherwise (a regular __cstring holds no literals). A final image's
# __TEXT,__text is code whatever its members.
cat <<EOF | $CC -o $t/data.o -c -xassembler -
.section __TEXT,__foo
.long 1
.section __TEXT,__bar,regular,pure_instructions
.long 1
EOF
cat <<EOF | $CC -o $t/code.o -c -xassembler -
.section __TEXT,__foo,regular,pure_instructions
.long 2
.section __TEXT,__bar
.long 2
EOF
cat <<EOF | $CC -o $t/std.o -c -xassembler -
.section __TEXT,__text
.long 3
.section __TEXT,__const
.long 3
.section __DATA,__data
.long 3
.section __TEXT,__stub_helper
.long 3
.section __TEXT,__cstring
.asciz "x"
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
set_flags $t/std.o __TEXT __text 0
set_flags $t/std.o __TEXT __const 0x80000400
set_flags $t/std.o __DATA __data 0x80000400
set_flags $t/std.o __TEXT __cstring 0x80000400

flags() {
  otool -l $1 | awk -v g=$2 -v s=$3 '$1 == "sectname" { n = $2 }
    $1 == "segname" { m = $2 } n == s && m == g && $1 == "flags" { print $2; exit }'
}

$CC --ld-path=$mold -o $t/exe $t/data.o $t/code.o $t/main.o
[ "$(flags $t/exe __TEXT __foo)" = 0x00000000 ]
[ "$(flags $t/exe __TEXT __bar)" = 0x80000400 ]
$t/exe

$CC --ld-path=$mold -o $t/exe2 $t/code.o $t/data.o $t/main.o
[ "$(flags $t/exe2 __TEXT __foo)" = 0x80000400 ]
[ "$(flags $t/exe2 __TEXT __bar)" = 0x00000000 ]
$t/exe2

$CC --ld-path=$mold -o $t/exe3 $t/std.o $t/main.o
[ "$(flags $t/exe3 __TEXT __text)" = 0x80000400 ]
[ "$(flags $t/exe3 __TEXT __const)" = 0x00000000 ]
[ "$(flags $t/exe3 __DATA __data)" = 0x00000000 ]
[ "$(flags $t/exe3 __TEXT __stub_helper)" = 0x80000400 ]
[ "$(flags $t/exe3 __TEXT __cstring)" = 0x80000400 ]
$t/exe3

$CC --ld-path=$mold -o $t/exe4 $t/data.o $t/main.o \
  -Wl,-rename_section,__TEXT,__foo,__TEXT,__text
[ "$(flags $t/exe4 __TEXT __text)" = 0x80000400 ]
$t/exe4

$mold -arch $ARCH -r -o $t/r.o $t/std.o
[ "$(flags $t/r.o __TEXT __text)" = 0x80000400 ]
[ "$(flags $t/r.o __TEXT __const)" = 0x00000000 ]
[ "$(flags $t/r.o __TEXT __cstring)" = 0x80000400 ]
