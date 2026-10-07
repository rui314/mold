#!/bin/bash
source "$(dirname "$0")"/common.inc

# __objc_selrefs goes in __DATA_CONST by default only in an image bound
# for the shared region, at any deployment target. -const_selrefs puts
# it there in any image (with __DATA_CONST, so not with -no_data_const),
# -no_const_selrefs keeps it in __DATA, and the last of them counts. The
# runtime registers the selectors of a read-only __objc_selrefs as well.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -mmacosx-version-min=11.0 -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
- (int)greet;
@end
@implementation Foo
- (int)greet { return 42; }
@end
int main() {
  Foo *f = [Foo new];
  printf("%d %s\n", [f greet], sel_getName(@selector(greet)));
}
EOF

seg() {
  otool -l $1 | grep -A1 'sectname __objc_selrefs' | awk '$1 == "segname" { print $2 }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation
[ "$(seg $t/exe)" = __DATA ]
$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -Wl,-const_selrefs
[ "$(seg $t/exe)" = __DATA_CONST ]
$RUN $t/exe | grep -q '^42 greet$'
$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -Wl,-const_selrefs \
  -Wl,-no_data_const
[ "$(seg $t/exe)" = __DATA ]
$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -Wl,-no_const_selrefs \
  -Wl,-const_selrefs
[ "$(seg $t/exe)" = __DATA_CONST ]

$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -framework Foundation \
  -Wl,-install_name,/usr/lib/libb.dylib -mmacosx-version-min=11.0
[ "$(seg $t/b.dylib)" = __DATA_CONST ]
$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -framework Foundation \
  -Wl,-install_name,/usr/lib/libb.dylib -Wl,-no_const_selrefs
[ "$(seg $t/b.dylib)" = __DATA ]
$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -framework Foundation \
  -Wl,-install_name,/usr/lib/libb.dylib -Wl,-const_selrefs -Wl,-no_const_selrefs
[ "$(seg $t/b.dylib)" = __DATA ]
