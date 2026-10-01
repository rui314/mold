#!/bin/bash
source "$(dirname "$0")"/common.inc

# A non-extern relocation names its target by section and address. A
# -r link re-targets one at the symbol at or before that address in its
# atom - an alt entry too - with the rest as the addend; where names
# share a place, the first as ld-prime ranks them: non-weak before weak,
# then external, private external and local, by descending name. A
# target no symbol precedes stays section-relative, as x86-64 objects
# refer to a label before a section's first symbol; on arm64 an ltmpN
# label names that place.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.data
.p2align 3
Lhead: .quad 0
_y: .quad 1
.globl _g
_g: .quad 2
_a:
_b: .quad 3
.globl _h
.private_extern _h
_h: .quad 4
.globl _alt
.alt_entry _alt
_alt: .quad 5
.globl _w
.weak_definition _w
_w:
_l: .quad 6
.globl _p
_p:
.quad _y + 8
.quad _b + 4
.quad _h + 8
.quad _h + 12
.quad _w
.quad _y + 4
.quad Lhead + 4
.subsections_via_symbols
EOF

# Makes the pointers' relocations section-relative, as another
# assembler might write them: the ordinal of the target's section, and
# its address in the field.
cat > $t/patch.py <<'EOF2'
import struct, sys
d = bytearray(open(sys.argv[1], 'rb').read())
off = 32
sects = []
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    if cmd == 0x19:
        sects += [off + 72 + i * 80 for i in range(struct.unpack_from('<I', d, off + 64)[0])]
    if cmd == 0x2:
        symoff = struct.unpack_from('<I', d, off + 8)[0]
    off += size
for s in sects:
    base, _, reloff, nreloc = struct.unpack_from('<IIII', d, s + 48)
    for i in range(nreloc):
        addr, info = struct.unpack_from('<II', d, reloff + 8 * i)
        if info >> 27 & 1:
            _, _, sect, _, val = struct.unpack_from('<IBBHQ', d, symoff + 16 * (info & 0xffffff))
            field = struct.unpack_from('<Q', d, base + addr)[0]
            struct.pack_into('<Q', d, base + addr, (field + val) % 2**64)
            struct.pack_into('<I', d, reloff + 8 * i + 4, sect | 3 << 25)
open(sys.argv[2], 'wb').write(d)
EOF2
python3 $t/patch.py $t/a.o $t/b.o
otool -rv $t/b.o > $t/log
[ "$(grep -c ' False  UNSIGND ' $t/log)" = 7 ]

$mold -r -arch $ARCH -o $t/c.o $t/b.o
otool -rv $t/c.o | awk '/UNSIGND/ { print $1, $5, $NF }' | sort > $t/relocs
if [ $ARCH = arm64 ]; then
  head='00000068 True ltmp1'
else
  head='00000068 False (__DATA,__data)'
fi
cat <<EOF > $t/expected
00000038 True _g
00000040 True _b
00000048 True _alt
00000050 True _alt
00000058 True _l
00000060 True _y
$head
EOF
diff $t/expected $t/relocs

# The fields now hold the offsets from those symbols.
cat <<EOF | $CC -o $t/main.o -c -xc -
extern char g[], alt[], w[], *p[];
int main() {
  return !(p[0] == g && p[1] == g + 12 && p[2] == alt && p[3] == alt + 4 &&
           p[4] == w && p[5] == g - 4 && p[6] == g - 12);
}
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/c.o
$t/exe

# One before its section's start is re-targeted at the section's
# first symbol, with a negative addend.
cat <<EOF | $CC -o $t/f.o -c -xassembler -
.text
.globl _fn
_fn: ret
.data
.p2align 3
.globl _d
_d: .quad 0
.globl _q
_q: .quad _d - 8
.subsections_via_symbols
EOF
python3 $t/patch.py $t/f.o $t/g.o
$mold -r -arch $ARCH -o $t/h.o $t/g.o
otool -rv $t/h.o | grep -q '^00000008 False ?( 3)  True   UNSIGND False     _d$'
cat <<EOF | $CC -o $t/main3.o -c -xc -
extern char d[], *q;
int main() { return q != d - 8; }
EOF
$CC --ld-path=$mold -o $t/exe3 $t/main3.o $t/h.o
$t/exe3

# x86-64 assemblers write such references themselves; a pc-relative
# field keeps its distance from the instruction's end, which may lie
# past the field (the immediate of the movl).
[ $ARCH = x86_64 ] || exit 0
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.text
.globl _get
_get:
  movl \$1, Ld+12(%rip)
  leaq Ld+12(%rip), %rax
  ret
.data
Ld: .quad 0
_z: .quad 0
.globl _zz
_zz: .quad 0
.subsections_via_symbols
EOF
$mold -r -arch $ARCH -o $t/e.o $t/d.o
otool -rv $t/e.o > $t/log
[ "$(grep -c ' True   SIGNED .* _z$' $t/log)" = 2 ]

cat <<EOF | $CC -o $t/main2.o -c -xc -
extern char zz[];
char *get(void);
int main() { return !(get() == zz - 4 && *(int *)(zz - 4) == 1); }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/main2.o $t/e.o
$t/exe2
