#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime knows the Objective-C runtime's sections by name and reads
# one of another type as the kind its name says: a class or category
# list typed as C strings is pointers still, __objc_const typed as
# literals is data not to merge, a regular __objc_methname is C strings
# to merge, and __objc_selrefs typed as C strings is selector
# references, in a final image and a -r output alike.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
- (int)bar;
@end
@implementation Foo
- (int)bar { return 42; }
@end
@interface Foo (Cat)
- (int)baz;
@end
@implementation Foo (Cat)
- (int)baz { return 7; }
@end
int main(void) {
  Foo *f = [Foo new];
  printf("%d %d\n", [f bar], [f baz]);
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__objc_methname,regular
.asciz "bar"
EOF

# Rewrites the flags of sections: arguments are FILE, then SEG SECT
# FLAGS for each section.
set_flags() {
  python3 - "$@" <<'EOF'
import struct, sys
path, args = sys.argv[1], sys.argv[2:]
want = {(args[i], args[i + 1]): int(args[i + 2], 0) for i in range(0, len(args), 3)}
d = bytearray(open(path, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        name = (d[s + 16:s + 32].rstrip(b'\0').decode(), d[s:s + 16].rstrip(b'\0').decode())
        if name in want:
            struct.pack_into('<I', d, s + 64, want[name])
    off += size
open(path, 'wb').write(d)
EOF
}

set_flags $t/a.o __DATA __objc_classlist 0x2 __DATA __objc_catlist 0x2 \
  __DATA __objc_selrefs 0x2 __DATA __objc_const 0x4 __TEXT __objc_methname 0
set_flags $t/b.o __TEXT __objc_methname 0

sect() {
  otool -l $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1; next }
    f && $1 == "size" { z = $2 } f && $1 == "flags" { print z, $2; f = 0 }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation
$t/exe | grep -q '^42 7$'
sect $t/exe __objc_selrefs | grep -q ' 0x00000000$'
sect $t/exe __objc_methname > $t/methname
grep -q ' 0x00000002$' $t/methname

# b.o's "bar" merges into a.o's.
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -framework Foundation
$t/exe2 | grep -q '^42 7$'
sect $t/exe2 __objc_methname | cmp - $t/methname

$mold -arch $ARCH -r -o $t/r.o $t/a.o
[ "$(sect $t/r.o __objc_classlist | cut -d' ' -f2)" = 0x10000000 ]
$CC --ld-path=$mold -o $t/exe3 $t/r.o -framework Foundation
$t/exe3 | grep -q '^42 7$'
