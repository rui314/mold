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
$t/exe | grep -q '^42 responds$'
# One slot per selector, as ld64 keeps it: the stub for
# componentsToRegister loads the compiler's selector reference rather
# than a synthesized slot of its own.
otool -l $t/exe | grep -A3 'sectname __objc_selrefs' | grep -q 'size 0x0000000000000008'

otool -l $t/exe > $t/lc
[ "$(grep -c 'sectname __objc_selrefs' $t/lc)" = 1 ]
[ "$(grep -c 'sectname __objc_methname' $t/lc)" = 1 ]
grep -q 'sectname __objc_stubs' $t/lc
