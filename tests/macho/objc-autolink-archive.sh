#!/bin/bash
source "$(dirname "$0")"/common.inc

# -ObjC loads the Objective-C members of the archives the command line
# names, not of one only an auto-link option names (a Swift object's
# `-framework X` for a static framework, Kickstarter's FBSDKShareKit) or
# only -possible-l: ld-prime loads such an archive for the symbols its
# members define, as without -ObjC. -all_load still loads all of it.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Unreferenced : NSObject @end
@implementation Unreferenced @end
EOF
echo 'int bar(void) { return 42; }' | $CC -o $t/b.o -c -xc -
rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/a.o $t/b.o
mkdir -p $t/Foo.framework
cp $t/libfoo.a $t/Foo.framework/Foo

echo 'int bar(void); int main() { return bar(); }' | $CC -o $t/main.o -c -xc -
echo '.linker_option "-lfoo"' | $CC -o $t/opt-l.o -c -xassembler -
echo '.linker_option "-framework", "Foo"' | $CC -o $t/opt-f.o -c -xassembler -

classes() {
  nm $t/exe | grep -c 'OBJC_CLASS_\$_Unreferenced'
}

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -lfoo -Wl,-ObjC -lobjc
[ "$(classes)" = 1 ]

$CC --ld-path=$mold -o $t/exe $t/main.o $t/opt-l.o -L$t -Wl,-ObjC -lobjc
[ "$(classes)" = 0 ]
nm $t/exe | grep -q ' T _bar$'

$CC --ld-path=$mold -o $t/exe $t/main.o $t/opt-f.o -F$t -Wl,-ObjC -lobjc
[ "$(classes)" = 0 ]

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-possible-lfoo -Wl,-ObjC -lobjc
[ "$(classes)" = 0 ]
nm $t/exe | grep -q ' T _bar$'

$CC --ld-path=$mold -o $t/exe $t/main.o $t/opt-l.o -L$t -Wl,-all_load -lobjc
[ "$(classes)" = 1 ]
