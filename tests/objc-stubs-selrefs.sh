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
$t/exe | grep '^42 responds$'
# One slot per selector, as ld64 keeps it: the stub for
# componentsToRegister loads the compiler's selector reference rather
# than a synthesized slot of its own.
otool -l $t/exe | grep -A3 'sectname __objc_selrefs' | grep 'size 0x0000000000000008'

otool -l $t/exe > $t/lc
[ "$(grep -c 'sectname __objc_selrefs' $t/lc)" = 1 ]
[ "$(grep -c 'sectname __objc_methname' $t/lc)" = 1 ]
grep -q 'sectname __objc_stubs' $t/lc
# A stub's selector name is an input's string of that name where there
# is one (ld-prime coalesces the two): each name appears once. Every
# input string here is a stub's selector, taken over by the synthesized
# one, so __objc_methname follows the input-derived __TEXT sections.
otool -X -s __TEXT __objc_methname -V $t/exe | sort | uniq -d > $t/dups
[ ! -s $t/dups ]
awk '$1 == "sectname" { s = $2; next }
  $1 == "segname" { if ($2 == "__TEXT" && s != "") print s; s = "" }' $t/lc > $t/text
[ "$(grep -A1 '^__cstring$' $t/text | tail -1)" = __objc_methname ]
# ld-prime packs x86-64's 13-byte stubs back to back, byte-aligned
# (arm64's are 32 bytes, 32-byte aligned).
if [ $ARCH = x86_64 ]; then
  grep -A8 'sectname __objc_stubs' $t/lc | grep 'align 2^0'
  grep -A4 'sectname __objc_stubs' $t/lc | grep 'size 0x000000000000000d'
else
  grep -A8 'sectname __objc_stubs' $t/lc | grep 'align 2^5'
  grep -A4 'sectname __objc_stubs' $t/lc | grep 'size 0x0000000000000020'
fi
