#!/bin/bash
source "$(dirname "$0")"/common.inc

# A non-extern relocation names its target section by ordinal and holds
# the target's address. The named section is taken whatever the
# address: one before its start or beyond its end is its first or last
# subsection's, and one just past its end its last subsection's.

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
import struct, sys, macho
src, dst = sys.argv[1], sys.argv[2]
vals = [int(v, 0) for v in sys.argv[3:]]
m = macho.MachO(src)
data = m.section('__data')
assert data.addr == 8 and data.nreloc == 3
for (roff, _, _), field, val in zip(m.relocs(data), (24, 16, 8), reversed(vals)):
    struct.pack_into('<II', m.data, roff, field, 2 | 3 << 25)
    m.set_u64(data.offset + field, val)
m.save(dst)
EOF2
python3 $t/patch.py $t/a.o $t/b.o 0x0 0x40 0x30

cat <<EOF | $CC -o $t/main.o -c -xc -
extern char d[], e[], *p1, *p2, *p3;
int main() {
  return !((unsigned long)p1 == (unsigned long)d - 8 && p2 == e + 24 && p3 == e + 8);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/b.o 2> /dev/null
$RUN $t/exe

# A relocatable link keeps the targets.
$mold -arch $ARCH -r -o $t/c.o $t/b.o 2> /dev/null
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/c.o 2> /dev/null
$RUN $t/exe2
