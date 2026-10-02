#!/bin/bash
source "$(dirname "$0")"/common.inc

echo hello > $t/blob

# The sections of -sectcreate and -add_empty_section sit next to the
# linker's own content, such as the lazy binder's __dyld_private word,
# without disturbing either.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { puts("x"); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-no_fixup_chains \
  -Wl,-sectcreate,__DATA,__blob,$t/blob
$t/exe | grep '^x$'
otool -s __DATA __blob $t/exe | grep -E '6c6c6568 6f 0a|68 65 6c 6c 6f 0a'
nm -m $t/exe | grep -F '(__DATA,__data) non-external __dyld_private'

# Or the selector names of arm64's objc_msgSend$ stubs: the stub for
# bar absorbs the input's name "bar", so __objc_methname is only
# synthesized.
[ $ARCH = arm64 ] || exit 0

cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
- (void)bar;
@end
@implementation Foo
- (void)bar { puts("bar"); }
@end
int main() { [[Foo new] bar]; }
EOF

$CC --ld-path=$mold -o $t/exe2 $t/b.o -framework Foundation \
  -Wl,-add_empty_section,__TEXT,__empty -Wl,-sectcreate,__TEXT,__blob,$t/blob
$t/exe2 | grep '^bar$'
otool -l $t/exe2 > $t/lc2
grep -A3 'sectname __empty' $t/lc2 | grep -E 'size 0x0+$'
otool -s __TEXT __blob $t/exe2 | grep -E '6c6c6568 6f 0a|68 65 6c 6c 6f 0a'
otool -X -s __TEXT __objc_methname -V $t/exe2 | grep -x bar
