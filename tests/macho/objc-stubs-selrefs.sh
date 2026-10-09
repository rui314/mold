#!/bin/bash
source "$(dirname "$0")"/common.inc

# The Objective-C runtime uniques the selectors of one __objc_selrefs
# section per image. With message sends compiled to
# _objc_msgSend$<selector> stubs, the linker synthesizes selector
# references of its own; if those form a second __objc_selrefs
# section, a compiler-emitted @selector() in the other one is never
# registered and respondsToSelector: fails for it (Firebase's +load
# checks broke this way in Sequel Ace).
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fobjc-msgsend-selector-stubs -mmacosx-version-min=13.0 -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
+ (int)componentsToRegister;
@end
@implementation Foo
+ (int)componentsToRegister { return 42; }
@end
int main() {
  // A message send through a linker stub, and a @selector() the
  // compiler emits as a selector reference.
  int v = [Foo componentsToRegister];
  BOOL ok = [Foo respondsToSelector:@selector(componentsToRegister)];
  printf("%d %s\n", v, ok ? "responds" : "MISSING");
  return 0;
}
EOF

$CC --ld-path=$mold -mmacosx-version-min=13.0 -o $t/exe $t/a.o -framework Foundation
$RUN $t/exe | grep '^42 responds$'
otool -l $t/exe > $t/lc
[ "$(grep -c 'sectname __objc_selrefs' $t/lc)" = 1 ]
[ "$(grep -c 'sectname __objc_methname' $t/lc)" = 1 ]
grep -q 'sectname __objc_stubs' $t/lc
# x86-64's stubs are 13 bytes, packed back to back, arm64's 32 bytes;
# the section is 32-byte aligned on either. (ld-prime leaves x86-64's
# byte-aligned.)
grep -A8 'sectname __objc_stubs' $t/lc | grep 'align 2^5'
if [ $ARCH = x86_64 ]; then
  grep -A4 'sectname __objc_stubs' $t/lc | grep 'size 0x000000000000000d'
else
  grep -A4 'sectname __objc_stubs' $t/lc | grep 'size 0x0000000000000020'
fi
