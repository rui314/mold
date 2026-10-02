#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output lists each object's local symbols in the order of that
# object's sections and of their addresses in each - a zerofill section
# goes by its ordinal, though its address is the object's highest -
# with the names at one address by rank: a private external, a local,
# a weak definition, each rank by descending name. The stabs follow,
# opened by an N_SO of their own, then the externals, and the string
# table holds the externals' names first (ld-prime).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.p2align 2
.private_extern _p1
_p1: ret
zz1:
aa1:
 ret
t1_first: ret
.zerofill __DATA,__bss,b1,8,3
.data
d1_second: .quad 1
.globl _main
.text
_main: ret
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
d2: .quad 2
.text
.p2align 2
t2: ret
.private_extern _p2
_p2: ret
.globl _f2
_f2: ret
.subsections_via_symbols
EOF
$mold -arch $ARCH -r $t/a.o $t/b.o -o $t/r.o
[ "$(nm -p $t/r.o | awk '$2 ~ /^[a-z]$/ && $3 !~ /^ltmp/ {printf "%s ", $3}')" = \
  '_p1 zz1 aa1 t1_first b1 d1_second t2 _p2 d2 ' ]

cat <<EOF | $CC -o $t/c.o -c -g -xc -
static int helper(void) { return 1; }
int one(void) { return helper(); }
EOF
$mold -arch $ARCH -r $t/c.o -o $t/r2.o
python3 - $t/r2.o <<'EOF2'
import struct, sys
d = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    if cmd == 0x2:
        symoff, nsyms, stroff, strsize = struct.unpack_from('<IIII', d, off + 8)
    off += size
ents = []
for i in range(nsyms):
    strx, typ, sect, desc, val = struct.unpack_from('<IBBHQ', d, symoff + i * 16)
    ents.append((strx, typ, d[stroff + strx:d.index(b'\0', stroff + strx)].decode()))
assert d[stroff:stroff + 2] == b' \0'
ents = [e for e in ents if not e[2].startswith('ltmp')]
assert ents[0][1] == 0x0e and ents[0][2] == '_helper', ents[0]
assert ents[1][1] == 0x64 and ents[1][2] == '', ents[1]
ext = [e for e in ents if e[1] == 0x0f]
assert ext and ext[0][0] == 2, ext
assert ents[-1][1] == 0x0f
EOF2
