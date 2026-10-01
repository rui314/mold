#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib's record marks what a merging link must not strip
# by what the objects' sections say: a reference to a superclass (for
# a message to super), in __objc_superrefs (no_dead_strip), is kept,
# as ld-prime records it; only a class reference, which it reads as
# the class it points at, isn't so marked.
cat <<EOF | $CC -o $t/a.o -c -O1 -xobjective-c -
#import <Foundation/Foundation.h>
@interface Base : NSObject
- (int)v;
@end
@implementation Base
- (int)v { return 1; }
@end
@interface Sub : Base
@end
@implementation Sub
- (int)v { return [super v] + 2; }
@end
int objc_entry(void) { return [[Sub new] v]; }
EOF

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int objc_entry(void);
int main() { printf("%d\n", objc_entry()); }
EOF

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -framework Foundation \
  -Wl,-make_mergeable -Wl,-install_name,@rpath/libfoo.dylib

# The flags of the record's entry named for the superclass reference:
# bit 17 is "don't dead strip".
python3 - $t/libfoo.dylib > $t/flags <<'EOF'
import struct, sys
data = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', data, 16)[0]):
    cmd, size, dataoff = struct.unpack_from('<III', data, off)
    if cmd == 0x36:
        b = data[dataoff:]
    off += size
nents, count = struct.unpack_from('<II', b, 0x60)
names, _ = struct.unpack_from('<II', b, 0x88)
for i in range(count):
    name, flags = struct.unpack_from('<II', b, nents + 40 * i + 12)
    if name == 0xffffff:
        continue
    at = names + 16 * name
    start = at + struct.unpack_from('<q', b, at)[0]
    if b[start:b.index(b'\0', start)] == b'l_OBJC_CLASSLIST_SUP_REFS_$_':
        print('dont-dead-strip' if flags >> 17 & 1 else 'strippable')
EOF
grep -q '^dont-dead-strip$' $t/flags

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo -Wl,-no_merged_libraries_hook \
  -Wl,-dead_strip
$t/exe | grep -q '^3$'
$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo -Wl,-no_merged_libraries_hook -Wl,-dead_strip
$t/exe2 | grep -q '^3$'
