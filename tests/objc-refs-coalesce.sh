#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime coalesces the __objc_superrefs and __objc_protorefs entries
# of one class or protocol but for those a symbol names - on arm64 the
# assembler's ltmp label of the section's start too. The compiler
# labels each entry, but an x86-64 -r output drops the labels, so two
# categories calling super on one class share an entry after it.
cat <<EOF | $CC -o $t/base.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Base : NSObject
@end
@interface Base (Cats)
- (BOOL)isEquala:(id)o;
- (BOOL)isEqualb:(id)o;
@end
@implementation Base
@end
int main() {
  Base *b = [Base new];
  printf("%d %d\n", [b isEquala:b], [b isEqualb:nil]);
}
EOF

for n in a b; do
  cat <<EOF | $CC -o $t/$n.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Base : NSObject
@end
@implementation Base (C$n)
- (BOOL)isEqual$n:(id)o { return [super isEqual:o]; }
@end
EOF
  $mold -r -arch $ARCH -o $t/r$n.o $t/$n.o
done

size() {
  otool -l $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1 }
    f && $1 == "size" { print $2; f = 0 }'
}

[ $ARCH = arm64 ] && n=0x0000000000000010 || n=0x0000000000000008
$CC --ld-path=$mold -o $t/exe $t/base.o $t/ra.o $t/rb.o -framework Foundation
$t/exe | grep -q '^1 0$'
[ "$(size $t/exe __objc_superrefs)" = $n ]

# Unlabeled entries of one target in two objects.
for n in c d; do
  cat <<EOF | $CC -o $t/$n.o -c -xassembler -
.section __DATA,__objc_protorefs,coalesced,no_dead_strip
.p2align 3
.quad _p
.quad _p
EOF
done
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.data
.globl _p
.p2align 3
_p: .quad 0
EOF

[ $ARCH = arm64 ] && n=0x0000000000000018 || n=0x0000000000000008
$CC --ld-path=$mold -o $t/exe2 $t/base.o $t/c.o $t/d.o $t/e.o -framework Foundation
[ "$(size $t/exe2 __objc_protorefs)" = $n ]
