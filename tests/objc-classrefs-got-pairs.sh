#!/bin/bash
source "$(dirname "$0")"/common.inc

# From macOS 15 on, a class reference folds into the GOT, and loads of
# a class the image defines relax to computing its address (adrp+add)
# with no slot left; but only if every adrp of the class's slot, in
# every object, is followed in its function by one @PAGEOFF use before
# the next: code that loads twice through one adrp (as -O0 code can)
# keeps a rebased GOT slot for the class, and then every load of it,
# even a well-paired one, reads that slot.
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

got_rebases() { dyld_info -fixups $1 | grep '__got' | grep -c rebase; }
foo_class_load() {
  otool -tV $1 | sed -n '/^_foo_class:/,/ret/p' | grep -Eo $'\t(ldr|add)\tx0' | cut -f2
}

$CC --ld-path=$mold -o $t/exe1 $t/a.o $t/b1.o -framework Foundation -mmacosx-version-min=15.0
$t/exe1 | grep -q '^Foo 1$'
[ "$(got_rebases $t/exe1)" = 0 ]
[ "$(foo_class_load $t/exe1)" = add ]

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b2.o -framework Foundation -mmacosx-version-min=15.0
$t/exe2 | grep -q '^Foo 1$'
[ "$(got_rebases $t/exe2)" = 1 ]
[ "$(foo_class_load $t/exe2)" = ldr ]

$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/b3.o -framework Foundation -mmacosx-version-min=15.0
$t/exe3 | grep -q '^Foo 1$'
[ "$(got_rebases $t/exe3)" = 1 ]
[ "$(foo_class_load $t/exe3)" = add ]
