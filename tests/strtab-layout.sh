#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime lays out a final image's string table after a leading " ":
# the external symbols' names, then the locals', then the debug notes'.
# Every entry has a copy of its own - two locals named alike get two -
# except that a note naming a symbol shares that symbol's string, but
# for the first local's (ld-prime copies that one again), and N_SO and
# N_OSO names are never shared.
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
# The externals' names come first.
assert ents[iext][0] == 2, ents[iext]
plain = [e for e in ents[:nlocal] if e[1] & 0xe0 == 0]
helpers = [e for e in plain if e[2] == '_helper']
assert len(helpers) == 2 and helpers[0][0] != helpers[1][0], helpers
assert min(e[0] for e in plain) > max(e[0] for e in ents[iext:])
stabs = ents[len(plain):nlocal]
funs = [e for e in stabs if e[1] == 0x24 and e[2]]
by_name = {e[2]: e[0] for e in ents[iext:iext + next_]}
for strx, _, name in funs:
    if name in by_name:
        assert strx == by_name[name], (name, strx)
# The first local's note gets a copy of its own; later locals' share.
first = ents[0]
assert [e for e in funs if e[2] == first[2]][0][0] != first[0]
later = [e for e in plain if e[2] == '_later'][0]
assert [e for e in funs if e[2] == '_later'][0][0] == later[0]
dirs = [e[0] for e in stabs if e[1] == 0x64 and e[2].endswith('/')]
assert len(dirs) == 2 and dirs[0] != dirs[1], dirs
EOF2
