#!/bin/bash
source "$(dirname "$0")"/common.inc

# From a deployment target of macOS 11, method lists are rewritten in
# the relative form ld64 emits (-objc_relative_method_lists): three
# 32-bit self-relative offsets per entry, the first to a selector
# reference slot, in read-only __TEXT,__objc_methlist with no fixups.
# Class, metaclass, category and protocol lists are all converted,
# and a selector no input references gets a reference synthesized.
cat <<EOF2 | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
#import <objc/message.h>
#import <stdio.h>
@protocol Greeter
- (int)greet;
+ (int)cgreet;
@optional
- (int)maybe;
@end
@interface Foo : NSObject <Greeter>
@end
@implementation Foo
- (int)greet { return 1; }
+ (int)cgreet { return 2; }
- (int)maybe { return 3; }
- (int)other:(int)x { return x; }
@end
@interface Foo (Extra)
- (int)extra;
+ (int)cextra;
@end
@implementation Foo (Extra)
- (int)extra { return 5; }
+ (int)cextra { return 6; }
@end
@interface NSObject (Ext2)
- (int)ext2;
@end
@implementation NSObject (Ext2)
- (int)ext2 { return 7; }
@end
int main() {
  Foo *f = [Foo new];
  // Selectors reached only by name: their references are synthesized.
  SEL other = sel_registerName("other:"), maybe = sel_registerName("maybe");
  printf("%d %d %d %d %d %d %d\n", [f greet], [Foo cgreet],
         ((int (*)(id, SEL, int))objc_msgSend)(f, other, 4),
         ((int (*)(id, SEL))objc_msgSend)(f, maybe),
         [f extra], [Foo cextra], [f ext2]);
  return 0;
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -mmacosx-version-min=15.0
$t/exe | grep -q '^1 2 4 3 5 6 7$'
otool -l $t/exe > $t/lc
grep -A1 'sectname __objc_methlist' $t/lc | grep -q 'segname __TEXT'
otool -ov $t/exe > $t/ov
# Every list is relative: none left in the classic 24-byte form.
[ "$(grep -c 'entsize 12 (relative)' $t/ov)" -ge 8 ]
not grep -q 'entsize 24' $t/ov
grep -q 'imp .*other:' $t/ov
grep -q 'imp .*cextra' $t/ov
nm -m $t/exe | grep '__objc_methlist)' | awk '{print $NF}' > $t/syms
# Category merging (on by default) names the list after Foo and Extra.
grep -q 'INSTANCE_METHODS_Foo(Extra)$' $t/syms
grep -q 'PROTOCOL_INSTANCE_METHODS_OPT_Greeter$' $t/syms
grep -q 'CATEGORY_INSTANCE_METHODS_NSObject_\$_Ext2$' $t/syms
# No fixups land in the lists.
dyld_info -fixups $t/exe > $t/fixups
not grep -q '__objc_methlist' $t/fixups

# Below macOS 11 the classic lists stay (arm64 macOS starts at 11, so
# only x86-64 can be built for 10.15).
if [ $ARCH = x86_64 ]; then
  $CC --ld-path=$mold -o $t/exe15 $t/a.o -framework Foundation -mmacosx-version-min=10.15
  $t/exe15 | grep -q '^1 2 4 3 5 6 7$'
  otool -l $t/exe15 > $t/lc15
  not grep -q '__objc_methlist' $t/lc15
  otool -ov $t/exe15 | grep -q 'entsize 24'
fi

$CC --ld-path=$mold -o $t/exe_no $t/a.o -framework Foundation -Wl,-no_objc_relative_method_lists
$t/exe_no | grep -q '^1 2 4 3 5 6 7$'
otool -l $t/exe_no > $t/lcno
not grep -q '__objc_methlist' $t/lcno
