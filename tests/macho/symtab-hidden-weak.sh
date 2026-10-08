#!/bin/bash
source "$(dirname "$0")"/common.inc

# A weak definition with hidden visibility (an inline function's
# linkonce_odr copy) is a private external; a final image lists it
# among its locals, and ld-prime keeps N_WEAK_DEF (0x80) in its n_desc.
cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((weak, visibility("hidden"))) int hidden_weak(void) { return 1; }
__attribute__((visibility("hidden"))) int hidden(void) { return 2; }
int main(void) { return hidden_weak() + hidden(); }
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o

python3 - $t/exe <<'EOF2'
import struct, sys
d = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    if cmd == 0x2:
        symoff, nsyms, stroff, _ = struct.unpack_from('<IIII', d, off + 8)
    off += size
desc = {}
for i in range(nsyms):
    strx, typ, sect, n_desc, val = struct.unpack_from('<IBBHQ', d, symoff + i * 16)
    desc[d[stroff + strx:d.index(b'\0', stroff + strx)]] = (typ, n_desc)
assert desc[b'_hidden_weak'] == (0x1e, 0x80), desc[b'_hidden_weak']
assert desc[b'_hidden'] == (0x1e, 0), desc[b'_hidden']
EOF2

# A -r output makes it local too, and keeps N_WEAK_DEF there as well.
$mold -arch $ARCH -r $t/a.o -o $t/r.o
python3 - $t/r.o <<'EOF2'
import struct, sys
d = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    if cmd == 0x2:
        symoff, nsyms, stroff, _ = struct.unpack_from('<IIII', d, off + 8)
    off += size
desc = {}
for i in range(nsyms):
    strx, typ, sect, n_desc, val = struct.unpack_from('<IBBHQ', d, symoff + i * 16)
    desc[d[stroff + strx:d.index(b'\0', stroff + strx)]] = (typ, n_desc)
assert desc[b'_hidden_weak'] == (0x1e, 0x80), desc[b'_hidden_weak']
EOF2
