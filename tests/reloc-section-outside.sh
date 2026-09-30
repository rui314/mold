#!/bin/bash
source "$(dirname "$0")"/common.inc

# A non-extern relocation names its target section by ordinal and holds
# the target's address. ld-prime takes the named section whatever the
# address: one before its start or beyond its end is its first or last
# atom's, with a warning, and one just past its end its last atom's.

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _f
_f: .quad 0
.data
.p2align 3
.globl _d
_d: .quad 0
.globl _p1
_p1: .quad _d
.globl _p2
_p2: .quad _d
.globl _p3
_p3: .quad _d
.globl _e
_e: .quad 0
.section __DATA,__more
.globl _m
_m: .quad 0
.subsections_via_symbols
EOF

# Makes _p1, _p2 and _p3 section-relative to __data (section 2), at the
# addresses given.
cat > $t/patch.py <<'EOF2'
import struct, sys
src, dst = sys.argv[1], sys.argv[2]
vals = [int(v, 0) for v in sys.argv[3:]]
d = bytearray(open(src, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        if d[s:s + 16].rstrip(b'\0') == b'__data':
            addr, _ = struct.unpack_from('<QQ', d, s + 32)
            base, _, reloff, nreloc = struct.unpack_from('<IIII', d, s + 48)
            assert addr == 8 and nreloc == 3
            for j, (field, val) in enumerate(zip((24, 16, 8), reversed(vals))):
                struct.pack_into('<II', d, reloff + 8 * j, field, 2 | 3 << 25)
                struct.pack_into('<Q', d, base + field, val)
    off += size
open(dst, 'wb').write(d)
EOF2
python3 $t/patch.py $t/a.o $t/b.o 0x0 0x40 0x30

cat <<EOF | $CC -o $t/main.o -c -xc -
extern char d[], e[], *p1, *p2, *p3;
int main() {
  return !((unsigned long)p1 == (unsigned long)d - 8 && p2 == e + 24 && p3 == e + 8);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/b.o 2> $t/log
$t/exe
grep -qF 'address=0x0 points before section(2) start and the target atom is ambiguous' $t/log
grep -qF 'address=0x40 points beyond section(2) end and the target atom is ambiguous' $t/log
not grep -q 'address=0x30' $t/log

# A relocatable link keeps the targets.
$mold -arch $ARCH -r -o $t/c.o $t/b.o 2> /dev/null
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/c.o 2> /dev/null
$t/exe2
