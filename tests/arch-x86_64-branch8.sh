#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = x86_64 ] || skip

# A one-byte branch (jmp rel8) to a symbol is a 1-byte pcrel BRANCH
# relocation. The assembler writes a jmp to another symbol as a 4-byte
# one, so the test rewrites it: relocation 0 of __text gets length 0,
# the jmp opcode 0xeb, and nops after its displacement byte.
cat > $t/patch.py <<'EOF2'
import struct, sys
src, dst = sys.argv[1], sys.argv[2]
d = bytearray(open(src, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        if d[s:s + 16].rstrip(b'\0') == b'__text':
            base = struct.unpack_from('<I', d, s + 48)[0]
            p = struct.unpack_from('<I', d, s + 56)[0]
    off += size
addr, w = struct.unpack_from('<iI', d, p)
struct.pack_into('<iI', d, p, addr, w & ~(3 << 25))
d[base + addr - 1] = 0xeb
struct.pack_into('<I', d, base + addr, 0x90909000)
open(dst, 'wb').write(d)
EOF2

# The jmp becomes one, with three nops after it.
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.text
.globl _main
_main:
  movl \$3, %eax
  jmp _g
  movl \$100, %eax
.globl _g
_g:
  addl \$4, %eax
  ret
.subsections_via_symbols
EOF
python3 $t/patch.py $t/e.o $t/e1.o
$CC --ld-path=$mold -o $t/exe $t/e1.o
code=0
$t/exe || code=$?
[ $code = 7 ]
$mold -r -arch $ARCH -o $t/r.o $t/e1.o
otool -rv $t/r.o > $t/relocs
grep -q 'True *byte *True *BRANCH' $t/relocs

# But it can't reach a symbol more than 127 bytes away, or one in a
# dylib.
cat <<EOF | $CC -o $t/f.o -c -xassembler -
.text
.globl _main
_main:
  movl \$3, %eax
  jmp _g
  .space 303, 0x90
.globl _g
_g:
  ret
.subsections_via_symbols
EOF
python3 $t/patch.py $t/f.o $t/f1.o
not $CC --ld-path=$mold -o $t/exe $t/f1.o 2> $t/log
grep -Eq "fixup error \(kind=x86_64_branch8\) at '_main'\+0x6 from f1.o, 8-bit branch out of range \(displacement=306, max is \+/-127\), from 0x[0-9A-F]+ to 0x[0-9A-F]+ \('_g'\)" $t/log

cat <<EOF | $CC -o $t/ext.o -c -xc -
int ext = 42;
EOF
$CC --ld-path=$mold -shared -o $t/libext.dylib $t/ext.o
cat <<EOF | $CC -o $t/g.o -c -xassembler -
.text
.globl _main
_main:
  jmp _ext
  ret
.subsections_via_symbols
EOF
python3 $t/patch.py $t/g.o $t/g1.o
not $CC --ld-path=$mold -o $t/exe $t/g1.o $t/libext.dylib 2> $t/log
grep -qF "fixup error (kind=x86_64_branch8) at '_main'+0x1 from g1.o, target '_ext' does not have address" $t/log
# It takes no stub, nor a GOT slot for one: the section layout
# printed with the error has neither.
grep -q '^final section layout:' $t/log
not grep -q '__stubs\|__got' $t/log
