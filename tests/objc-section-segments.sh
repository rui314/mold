#!/bin/bash
source "$(dirname "$0")"/common.inc

# The Objective-C runtime's sections stay in the segment an input puts
# them in, __DATA_CONST or another one, unless they are __DATA's, which
# may move to __DATA_CONST. In a final image they are plain data
# wherever they are: the runtime finds them by name, and their
# no-dead-strip, literal-pointer or coalesced bits direct the linker
# alone. (ld-prime keeps those bits for the sections in __DATA.)
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
[ "$(flags $t/exe __DATA __objc_selrefs)" = 0x00000000 ]

$CC --ld-path=$mold -o $t/exe2 $t/b.o -framework Foundation -Wl,-no_data_const
$t/exe2 | grep -q '^3 P$'
[ "$(flags $t/exe2 __DATA __objc_protolist)" = 0x00000000 ]
[ "$(flags $t/exe2 __DATA __objc_protorefs)" = 0x00000000 ]
[ "$(flags $t/exe2 __DATA __objc_classlist)" = 0x00000000 ]
