#!/bin/bash
source "$(dirname "$0")"/common.inc

echo hello > $t/blob

# ld-prime reads the sections of -sectcreate and -add_empty_section as
# inputs and makes its own content after all inputs, so the options'
# sections come before a section of the same rank that holds only the
# linker's content, such as the lazy binder's __dyld_private word.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { puts("x"); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-no_fixup_chains \
  -Wl,-sectcreate,__DATA,__blob,$t/blob
otool -l $t/exe | grep 'sectname __' | awk '{print $2}' | tr '\n' ' ' > $t/log
grep -q '__blob __data ' $t/log

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
- (void)bar {}
@end
int main() { [[Foo new] bar]; }
EOF

$CC --ld-path=$mold -o $t/exe2 $t/b.o -framework Foundation \
  -Wl,-add_empty_section,__TEXT,__empty -Wl,-sectcreate,__TEXT,__blob,$t/blob
otool -l $t/exe2 | grep 'sectname __' | awk '{print $2}' | tr '\n' ' ' > $t/log2
grep -q '__empty __blob __objc_methname ' $t/log2
