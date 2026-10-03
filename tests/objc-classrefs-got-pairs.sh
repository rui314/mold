#!/bin/bash
source "$(dirname "$0")"/common.inc

# Code loads a class through its __objc_classrefs slot: an adrp and an
# ldr of each load, or one adrp for two loads (as -O0 code can), and it
# may take the slot's address. Each works at any deployment target.
# (From macOS 15 on, ld-prime folds the slots into __got and relaxes
# the loads of a class the image defines where every adrp pairs with
# one load.)
[ "$ARCH" = arm64 ] || skip

cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#import <stdio.h>
@interface Foo : NSObject
@end
@implementation Foo
@end
Class foo_class(void) { return [Foo class]; }
Class load_twice(Class *second);
int main() {
  Class second = 0;
  Class first = load_twice(&second);
  printf("%s %d\n", class_getName(foo_class()), first == second && first == foo_class());
}
EOF

# load_twice loads the class twice: through an adrp each (b1 and b3),
# or through one adrp (b2). b3 also takes the slot's address, which
# needs the slot but relaxes nothing less.
for v in 1 2 3; do
  if [ $v = 2 ]; then second=''; else second='adrp x8, Lref@PAGE'; fi
  if [ $v = 3 ]; then addr='.globl _slot_addr
_slot_addr:
  adrp x8, Lref@PAGE
  add x0, x8, Lref@PAGEOFF
  ret'; else addr=''; fi
  cat <<EOF | $CC -o $t/b$v.o -c -xassembler -
.section __DATA,__objc_classrefs,regular,no_dead_strip
.p2align 3
Lref: .quad _OBJC_CLASS_\$_Foo
.text
.globl _load_twice
.p2align 2
_load_twice:
  adrp x8, Lref@PAGE
  ldr x9, [x8, Lref@PAGEOFF]
  $second
  ldr x10, [x8, Lref@PAGEOFF]
  str x10, [x0]
  mov x0, x9
  ret
$addr
.subsections_via_symbols
EOF
done

for v in 1 2 3; do
  $CC --ld-path=$mold -o $t/exe$v $t/a.o $t/b$v.o -framework Foundation -mmacosx-version-min=15.0
  $t/exe$v | grep -q '^Foo 1$'
  otool -l $t/exe$v | grep -q 'sectname __objc_classrefs'
done
