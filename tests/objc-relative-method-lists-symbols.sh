#!/bin/bash
source "$(dirname "$0")"/common.inc

# A method list rewritten in the relative form moves to
# __TEXT,__objc_methlist, and the input list's symbol names it there,
# a private external such as Swift's protocol method lists demoted as
# any other. (ld-prime lists it as a plain local, the list being a
# subsection of its own making. An x86-64 executable keeps its lists
# absolute, a dylib doesn't.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__objc_methname,cstring_literals
Lsel: .asciz "foo"
.section __TEXT,__objc_methtype,cstring_literals
Ltype: .asciz "v16@0:8"
.section __TEXT,__objc_classname,cstring_literals
Lname: .asciz "P"
.section __DATA,__objc_data
.p2align 3
.globl __PROTOCOL_INSTANCE_METHODS_P
.private_extern __PROTOCOL_INSTANCE_METHODS_P
.weak_definition __PROTOCOL_INSTANCE_METHODS_P
__PROTOCOL_INSTANCE_METHODS_P:
.long 24, 1
.quad Lsel, Ltype, 0
.section __DATA,__objc_const
.p2align 3
__PROTOCOL_P:
.quad 0, Lname, 0, __PROTOCOL_INSTANCE_METHODS_P, 0, 0, 0, 0
.long 80, 0
.quad 0, 0, 0
.section __DATA,__objc_protolist,coalesced,no_dead_strip
.p2align 3
.quad __PROTOCOL_P
.subsections_via_symbols
EOF
$CC --ld-path=$mold -o $t/a.dylib -dynamiclib $t/a.o -mmacosx-version-min=12.0
nm -m $t/a.dylib > $t/nm
grep -q '(__TEXT,__objc_methlist) non-external (was a private external) __PROTOCOL_INSTANCE_METHODS_P$' $t/nm

cat <<EOF | $CC -g -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
@end
@implementation Foo
- (int)bar { return 3; }
@end
int bar(void) { return [[Foo new] bar]; }
EOF
$CC --ld-path=$mold -o $t/b.dylib -dynamiclib $t/b.o -framework Foundation \
  -mmacosx-version-min=12.0
echo 'int bar(void); int main() { return bar() == 3 ? 0 : 1; }' | $CC -o $t/main.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/main.o $t/b.dylib
$t/exe
nm -ap $t/b.dylib > $t/nm2
grep -q ' s __OBJC_\$_INSTANCE_METHODS_Foo$' $t/nm2
