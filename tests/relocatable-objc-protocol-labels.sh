#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image names none of the entries of the Objective-C list and
# reference sections by their local labels, but keeps a private
# external there (clang's hidden __OBJC_LABEL_PROTOCOL_$_P in
# __objc_protolist and __OBJC_PROTOCOL_REFERENCE_$_P in
# __objc_protorefs). ld-prime keeps them as well after a -r link has
# demoted them to locals: the locals keep N_PEXT to say so.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
@protocol P
- (int)pm;
@end
@interface Foo : NSObject <P>
@end
@implementation Foo
- (int)pm { return 7; }
@end
int main() {
  printf("%s %d\n", protocol_getName(@protocol(P)), [[Foo new] pm]);
}
EOF

$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/nm-r
grep -q '(__DATA,__objc_protolist) non-external (was a private external) .*__OBJC_LABEL_PROTOCOL_\$_P$' $t/nm-r
grep -q '(__DATA,__objc_protorefs) non-external (was a private external) .*__OBJC_PROTOCOL_REFERENCE_\$_P$' $t/nm-r

$CC --ld-path=$mold -o $t/exe $t/r.o -framework Foundation
$t/exe | grep -q '^P 7$'
nm $t/exe > $t/nm
grep -q ' __OBJC_LABEL_PROTOCOL_\$_P$' $t/nm
grep -q ' __OBJC_PROTOCOL_REFERENCE_\$_P$' $t/nm
