#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image's string table starts with " \0", and every symbol has
# a string of its own - two locals named alike get two - but a debug
# note naming a symbol shares that symbol's string, so that a -g link
# doesn't write each name twice. N_SO and N_OSO names are never shared.
# (ld-prime gives the first local's note a copy of its own.)
cat <<EOF | $CC -o $t/a.o -c -g -xc -
static int helper(void) { return 1; }
static int later(void) { return 2; }
int one(void) { return helper() + later(); }
EOF
cat <<EOF | $CC -o $t/b.o -c -g -xc -
static int helper(void) { return 3; }
int main(void) { return helper(); }
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o

python3 - $t/exe <<'EOF2'
import struct, sys
d = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    if cmd == 0x2:
        symoff, nsyms, stroff, strsize = struct.unpack_from('<IIII', d, off + 8)
    if cmd == 0xb:
        ilocal, nlocal, iext, next_, iundef, nundef = struct.unpack_from('<6I', d, off + 8)
    off += size
ents = []
for i in range(nsyms):
    strx, typ, sect, desc, val = struct.unpack_from('<IBBHQ', d, symoff + i * 16)
    name = d[stroff + strx:d.index(b'\0', stroff + strx)].decode()
    ents.append((strx, typ, name))
assert d[stroff:stroff + 2] == b' \0'
syms = [e for e in ents if e[1] & 0xe0 == 0]
helpers = [e for e in syms if e[2] == '_helper']
assert len(helpers) == 2 and helpers[0][0] != helpers[1][0], helpers
assert len({e[0] for e in syms}) == len(syms), syms
stabs = [e for e in ents if e[1] & 0xe0 != 0]
funs = [e for e in stabs if e[1] == 0x24 and e[2]]
strx_of = {e[0] for e in syms}
for strx, _, name in funs:
    assert strx in strx_of, (name, strx)
dirs = [e[0] for e in stabs if e[1] == 0x64 and e[2].endswith('/')]
assert len(dirs) == 2 and dirs[0] != dirs[1], dirs
EOF2
