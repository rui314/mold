#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib's record header names the largest entry kind,
# content type, generic fixup kind and target fixup kind it uses; a
# merging link refuses a record that uses ones it doesn't know, as
# ld-prime does, in its words.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void) { return 3; }
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
int foo(void);
int main() { return foo(); }
EOF
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib

# patch OFFSET VALUE: a copy of the dylib with the 16-bit header field
# at OFFSET set to VALUE, in $t/p/libfoo.dylib.
patch() {
  mkdir -p $t/p
  python3 - $t/libfoo.dylib $t/p/libfoo.dylib $1 $2 <<'EOF'
import struct, sys
data = bytearray(open(sys.argv[1], 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', data, 16)[0]):
    cmd, size, dataoff = struct.unpack_from('<III', data, off)
    if cmd == 0x36:
        base = dataoff
    off += size
at, value = int(sys.argv[3]), int(sys.argv[4])
if at < 14:
    data[base + at] = value
else:
    struct.pack_into('<H', data, base + at, value)
open(sys.argv[2], 'wb').write(data)
EOF
}

check() {
  patch $1 $2
  not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/p -Wl,-merge-lfoo 2> $t/log
  grep -q "$3 in '$t/p/libfoo.dylib'" $t/log
}

check 12 25 'atom file uses unknown atom kind (25).  Max supported is 19'
check 13 90 'atom file uses unknown atom content type (90).  Max supported is 81'
check 14 20 'atom file uses unknown fixup kind (20).  Max supported is 14'
check 16 147 'atom file uses unknown fixup kind (147).  Max supported is 146'
check 16 266 'atom file uses unknown fixup kind (266).  Max supported is 265'
check 16 5 'unexpected generic fixup group'
check 16 512 'unknown fixup group'

# The largest ones known pass.
patch 12 19
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t/p -Wl,-merge-lfoo
