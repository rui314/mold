#!/bin/bash
source "$(dirname "$0")"/common.inc

# A non-extern relocation names its target by section and address. A
# -r output keeps it so, with the address moved to the merged layout,
# and a later link finds the subsection by the address as this one did.
# (ld-prime re-targets one at the symbol at or before the address.)
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
otool -rv $t/c.o > $t/log2
[ "$(grep -c ' False  UNSIGND False     1 (__DATA,__data)$' $t/log2)" = 7 ]

# The fields still point where they did.
cat <<EOF | $CC -o $t/main.o -c -xc -
extern char g[], alt[], w[], *p[];
int main() {
  return !(p[0] == g && p[1] == g + 12 && p[2] == alt && p[3] == alt + 4 &&
           p[4] == w && p[5] == g - 4 && p[6] == g - 12);
}
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/c.o
$RUN $t/exe

# So does one before its section's start.
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
otool -rv $t/h.o | grep -q '^00000008 False ?( 3)  False  UNSIGND False     2 (__DATA,__data)$'
cat <<EOF | $CC -o $t/main3.o -c -xc -
extern char d[], *q;
int main() { return q != d - 8; }
EOF
$CC --ld-path=$mold -o $t/exe3 $t/main3.o $t/h.o
$RUN $t/exe3

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
[ "$(grep -c ' False  SIGNED .* (__DATA,__data)$' $t/log)" = 2 ]

cat <<EOF | $CC -o $t/main2.o -c -xc -
extern char zz[];
char *get(void);
int main() { return !(get() == zz - 4 && *(int *)(zz - 4) == 1); }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/main2.o $t/e.o
$RUN $t/exe2
