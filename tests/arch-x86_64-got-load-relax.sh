#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = x86_64 ] || skip

# A GOT_LOAD (movq sym@GOTPCREL(%rip)) of a symbol defined in the image
# relaxes: ld-prime rewrites the movq into a leaq of the symbol and
# takes a leaq as one already, neither keeping a GOT slot, but refuses
# any other instruction, naming the fixup's kind, the subsection and the
# object's leaf name. A thread-local's TLV load relaxes the same way.
# A load of a dylib's symbol keeps its slot, whatever the instruction.

# Rewrites the opcode byte of the instruction under the first
# relocation of type TYPE in __text: the byte 2 before the field.
cat > $t/opcode.py <<'EOF2'
import struct, sys
src, dst, rtype, opcode = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4], 0)
d = bytearray(open(src, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        if d[s:s + 16].rstrip(b'\0') == b'__text':
            base, _, reloff, nreloc = struct.unpack_from('<IIII', d, s + 48)
            addrs = [struct.unpack_from('<I', d, reloff + 8 * j)[0] for j in range(nreloc)
                     if struct.unpack_from('<I', d, reloff + 8 * j + 4)[0] >> 28 == rtype]
            d[base + min(addrs) - 2] = opcode
    off += size
open(dst, 'wb').write(d)
EOF2

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _get
_get:
  movq _val@GOTPCREL(%rip), %rax
  ret
.globl _main
_main:
  callq _get
  movl (%rax), %eax
  ret
.data
.globl _val
_val: .long 42
.subsections_via_symbols
EOF

# leaq: the address of _val itself, with no GOT.
python3 $t/opcode.py $t/a.o $t/lea.o 3 0x8d
$CC --ld-path=$mold -o $t/exe $t/lea.o
code=0
$RUN $t/exe || code=$?
[ $code = 42 ]
otool -l $t/exe > $t/lc
not grep -q 'sectname __got' $t/lc

# addq: refused.
python3 $t/opcode.py $t/a.o $t/add.o 3 0x03
not $CC --ld-path=$mold -o $t/exe2 $t/add.o 2> $t/log
grep -qF "$t/add.o: _get+0x3: GOT load fixup does not point to a movq instruction" $t/log

# The same load of a dylib's symbol goes through the GOT.
cat <<EOF | $CC -o $t/b.o -c -xc -
int val = 42;
EOF
$CC --ld-path=$mold -shared -o $t/libb.dylib $t/b.o
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl _get
_get:
  movq _val@GOTPCREL(%rip), %rax
  ret
.subsections_via_symbols
EOF
python3 $t/opcode.py $t/c.o $t/c-add.o 3 0x03
$CC --ld-path=$mold -shared -o $t/libc.dylib $t/c-add.o $t/libb.dylib

# A TLV load of a thread-local defined in the image.
cat <<EOF | $CC -o $t/d.o -c -xc -
_Thread_local int tv = 5;
int get_tv(void) { return tv; }
EOF
python3 $t/opcode.py $t/d.o $t/d-add.o 9 0x03
not $CC --ld-path=$mold -shared -o $t/libd.dylib $t/d-add.o 2> $t/log
grep -q "$t/d-add.o: _get_tv+0x[0-9a-f]*: GOT load fixup does not point to a movq instruction" $t/log
