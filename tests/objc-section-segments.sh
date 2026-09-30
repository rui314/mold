#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime's rules for the flags of the Objective-C runtime's sections -
# no-dead-strip lists, selector references that stay literal pointers,
# a protocol list that stays coalesced in __DATA - hold for the
# sections of those names in __DATA, where compilers put them. One of
# an input's __DATA_CONST or another segment is data like any other.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA_CONST,__objc_classlist,regular,no_dead_strip
.p2align 3
.quad _x
.section __FOO,__objc_nlclslist,regular,no_dead_strip
.p2align 3
.quad _x
.section __DATA_CONST,__objc_selrefs,regular,no_dead_strip
.p2align 3
.quad _x
.data
.globl _x
_x: .quad 0
EOF

cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
@protocol P
- (int)f;
@end
@interface C : NSObject <P>
@end
@implementation C
- (int)f { return 3; }
@end
int main() {
  printf("%d %s\n", [[C new] f], protocol_getName(@protocol(P)));
}
EOF

flags() {
  otool -l $1 | awk -v g=$2 -v s=$3 '$1 == "sectname" { n = $2 }
    $1 == "segname" && n == s && $2 == g { f = 1 } f && $1 == "flags" { print $2; f = 0 }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -framework Foundation
$t/exe | grep -q '^3 P$'
[ "$(flags $t/exe __DATA_CONST __objc_classlist)" = 0x00000000 ]
[ "$(flags $t/exe __FOO __objc_nlclslist)" = 0x00000000 ]
[ "$(flags $t/exe __DATA_CONST __objc_selrefs)" = 0x00000000 ]
[ "$(flags $t/exe __DATA __objc_selrefs)" = 0x10000005 ]

$CC --ld-path=$mold -o $t/exe2 $t/b.o -framework Foundation -Wl,-no_data_const
$t/exe2 | grep -q '^3 P$'
[ "$(flags $t/exe2 __DATA __objc_protolist)" = 0x0000000b ]
[ "$(flags $t/exe2 __DATA __objc_protorefs)" = 0x1000000b ]
[ "$(flags $t/exe2 __DATA __objc_classlist)" = 0x10000000 ]
