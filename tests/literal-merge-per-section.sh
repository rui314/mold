#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime coalesces identical literals within a section, never across
# two: the class name "Foo" in __objc_classname stays although
# __cstring has a "Foo" too, and so does the selector name "length" in
# __objc_methname next to the "length" of __cstring.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
@interface Foo : NSObject
@end
@implementation Foo
- (unsigned long)length { return 6; }
@end
const char *name = "Foo";
const char *sel = "length";
int main() {
  printf("%s %s %s %lu\n", class_getName([Foo class]), name, sel,
         (unsigned long)[[Foo new] length]);
}
EOF

strings_of() { otool -v -s __TEXT $2 $1 | tail -n +3 | awk '{print $2}'; }

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation
$t/exe | grep -q '^Foo Foo length 6$'
strings_of $t/exe __objc_classname | grep -q '^Foo$'
strings_of $t/exe __objc_methname | grep -q '^length$'
strings_of $t/exe __cstring > $t/cstrings
grep -q '^Foo$' $t/cstrings
grep -q '^length$' $t/cstrings

$mold -r -arch $ARCH -o $t/r.o $t/a.o
strings_of $t/r.o __objc_classname | grep -q '^Foo$'
strings_of $t/r.o __objc_methname | grep -q '^length$'
