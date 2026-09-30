#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>
void hello() { printf("Hello world\n"); }
EOF

# ld-prime ignores -mark_dead_strippable_dylib with a warning, which
# a -w anywhere silences, and sets no MH_DEAD_STRIPPABLE_DYLIB.
$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o -Wl,-mark_dead_strippable_dylib 2> $t/log
grep -q -- '-mark_dead_strippable_dylib is obsolete' $t/log
otool -hv $t/b.dylib > $t/log
not grep -q DEAD_STRIPPABLE_DYLIB $t/log

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o -Wl,-mark_dead_strippable_dylib -Wl,-w 2> $t/log
not grep -q obsolete $t/log

# Nor does it drop a dylib whose header has the flag, which ld64 did
# when nothing bound to it.
python3 - $t/b.dylib $t/c.dylib <<'EOF'
import struct, sys
data = bytearray(open(sys.argv[1], 'rb').read())
flags = struct.unpack_from('<I', data, 24)[0]
struct.pack_into('<I', data, 24, flags | 0x400000)
open(sys.argv[2], 'wb').write(data)
EOF

cat <<EOF | $CC -o $t/d.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/d.o $t/c.dylib
objdump --macho --dylibs-used $t/exe | grep -Fq b.dylib
