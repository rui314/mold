#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 places each atom at an offset congruent, modulo its section's
# alignment, to the offset it had in its input section - an 8-byte
# atom at offset 8 of a 16-aligned section stays 8 mod 16 - instead of
# rounding every atom up to the section's alignment. The two differ
# once an atom's predecessor is gone (dead-stripped here): ld64 pads 8
# bytes to keep _b at 8 mod 16 after the 16-byte _a, and never pads
# when the predecessor is present. Rounding every atom up costs half
# the alignment per atom on average: NetNewsWire's __TEXT,__const came
# out 11KB (12%) larger than ld-prime's.
cat <<EOF2 | $CC -o $t/a.o -c -xassembler -
.subsections_via_symbols
.section __TEXT,__const
.p2align 4
.globl _a
_a: .quad 1, 2
EOF2
cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.subsections_via_symbols
.section __TEXT,__const
.p2align 4
_z: .quad 3
.globl _b
_b: .quad 4
.globl _c
_c: .long 5
.p2align 3
.globl _d
_d: .quad 6
EOF2
cat <<EOF2 | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern long a[2], b, d;
extern int c;
int main() { printf("%ld\\n", a[0] + b + c + d); }
EOF2

# Predecessors present: no padding beyond the input layout.
$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o $t/b.o
nm -n $t/exe | grep -E ' _(a|z|b|c|d)$' | awk '{print $1}' > $t/addrs
python3 - $t/addrs <<'EOF2'
import sys
a, z, b, c, d = [int(l, 16) for l in open(sys.argv[1])]
assert z == a + 16 and b == z + 8 and c == b + 8 and d == c + 8, [hex(x) for x in (a, z, b, c, d)]
assert b % 16 == 8 and d % 16 == 8
EOF2

# _z dead-stripped: _b keeps 8 mod 16 (8 bytes of padding after _a).
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o $t/b.o -Wl,-dead_strip
nm -n $t/exe2 | grep -q ' _z$' && exit 1
nm -n $t/exe2 | grep -E ' _(a|b|c|d)$' | awk '{print $1}' > $t/addrs2
python3 - $t/addrs2 <<'EOF2'
import sys
a, b, c, d = [int(l, 16) for l in open(sys.argv[1])]
assert b == a + 24 and b % 16 == 8, hex(b - a)
assert c == b + 8 and d == c + 8
EOF2
$t/exe2 | grep -q '^16$'
