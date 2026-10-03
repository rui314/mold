#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib's record header has flags (at 0x28) of what its
# objects had: bit 26 an Objective-C image info, which ld-prime's merge
# reads, and bit 31 classes (with one), by which mold decides whether a
# merging image needs the hook for the classes of mergeable libraries.
# Selectors, protocols, CFStrings or categories alone don't set it.
flags() {
  python3 - $1 <<'EOF'
import struct, sys
data = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', data, 16)[0]):
    cmd, size, dataoff = struct.unpack_from('<III', data, off)
    if cmd == 0x36:
        flags = struct.unpack_from('<Q', data, dataoff + 0x28)[0]
        print(flags >> 26 & 1, flags >> 31 & 1)
    off += size
EOF
}

cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@protocol P
- (int)p;
@end
SEL getsel(void) { return @selector(description); }
Protocol *getp(void) { return @protocol(P); }
NSString *s(void) { return @"x"; }
EOF
$CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -framework Foundation -Wl,-make_mergeable
[ "$(flags $t/a.dylib)" = '1 0' ]

cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface NSString (X)
- (int)x;
@end
@implementation NSString (X)
- (int)x { return 1; }
@end
EOF
$CC --ld-path=$mold -shared -o $t/b.dylib $t/b.o -framework Foundation -Wl,-make_mergeable
[ "$(flags $t/b.dylib)" = '1 0' ]

cat <<EOF | $CC -o $t/c.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface C : NSObject
@end
@implementation C
@end
EOF
$CC --ld-path=$mold -shared -o $t/c.dylib $t/c.o -framework Foundation -Wl,-make_mergeable
[ "$(flags $t/c.dylib)" = '1 1' ]

cat <<EOF | $CC -o $t/d.o -c -xassembler -
.section __TEXT,__swift5_types
.p2align 2
_d: .long 0
.subsections_via_symbols
EOF
$CC --ld-path=$mold -shared -o $t/d.dylib $t/d.o -Wl,-make_mergeable
[ "$(flags $t/d.dylib)" = '0 0' ]
