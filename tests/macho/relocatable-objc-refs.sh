#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 -r coalesces the per-object Objective-C reference records:
# __objc_selrefs entries naming the same selector, __objc_classrefs
# entries naming the same class, and identical __cfstring constants
# keep one copy (NetNewsWire's RSCore prelink had 56 class references
# where ld-prime's has 30). We kept every object's copy.
for i in 1 2; do
cat <<EOF2 | $CC -O2 -fobjc-arc -fno-asynchronous-unwind-tables -fno-exceptions -o $t/$i.o -c -xobjective-c -
#import <Foundation/Foundation.h>
Class cls$i(void) { return [NSObject class]; }
SEL sel$i(void) { return @selector(count); }
SEL selx$i(void) { return @selector(only$i); }
NSString *cf$i(void) { return @"shared"; }
NSString *cfx$i(void) { return @"only$i"; }
EOF2
done
$mold -r -arch $ARCH -o $t/r.o $t/1.o $t/2.o
size() { otool -l $t/r.o | grep -A3 "sectname $1" | grep size | awk '{print $2}'; }
[ "$(size __objc_classrefs)" = 0x0000000000000008 ]
[ "$(size __objc_selrefs)" = 0x0000000000000018 ]
[ "$(size __cfstring)" = 0x0000000000000060 ]
# The merged object works: both objects' references resolve.
cat <<EOF2 | $CC -O2 -fobjc-arc -o $t/main.o -c -xobjective-c -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
Class cls1(void); Class cls2(void); SEL sel1(void); SEL sel2(void); SEL selx1(void); SEL selx2(void);
NSString *cf1(void); NSString *cf2(void); NSString *cfx1(void); NSString *cfx2(void);
int main() {
  return (cls1() == cls2() && sel1() == sel2() && selx1() != selx2()
          && cf1() == cf2() && [cfx1() isEqualToString:@"only1"] && [cfx2() isEqualToString:@"only2"]) ? 0 : 1;
}
EOF2
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -framework Foundation
$t/exe

# A final link coalesces the selector references and CFString
# constants the same way (NetNewsWire's debug dylib had 592 selector
# references more than ld-prime's); class references become GOT
# slots there instead.
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/1.o $t/2.o -framework Foundation
$t/exe2
# Every selector reference slot names a different selector string.
dyld_info -fixups $t/exe2 | grep '__objc_selrefs' | awk '{print $NF}' > $t/seltargets
[ "$(wc -l < $t/seltargets)" = "$(sort -u $t/seltargets | wc -l)" ]
# "shared", "only1" and "only2" once each (main.o's copies of the
# latter two fold as well): three constants.
otool -l $t/exe2 | grep -A3 'sectname __cfstring' | grep -q 'size 0x0000000000000060'
