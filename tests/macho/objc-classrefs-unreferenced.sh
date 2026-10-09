#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# __objc_classrefs is taken one 8-byte class pointer at a time,
# whatever the labels, keeping one per class. From macOS 15 on the
# references to those slots become GOT references, and what folds is
# what is referenced: a slot nothing refers to stays in
# __objc_classrefs (and its class gets no GOT entry). -dead_strip
# removes such a slot although __objc_classrefs is no-dead-strip.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#import <stdio.h>
@interface Foo : NSObject
@end
@implementation Foo
@end
Class get_foo(void);
int main() { printf("%s %d\n", class_getName(get_foo()), get_foo() == [Foo class]); }
EOF

if [ "$ARCH" = arm64 ]; then
  load='adrp x8, l_pair@PAGE
  ldr x0, [x8, l_pair@PAGEOFF]
  ret'
else
  load='movq l_pair(%rip), %rax
  retq'
fi

# l_pair is one subsection of two slots, the second as unreferenced as
# l_date, its copy.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _get_foo
.p2align 2
_get_foo:
  $load
.section __DATA,__objc_classrefs,regular,no_dead_strip
.p2align 3
l_pair:
  .quad _OBJC_CLASS_\$_Foo
  .quad _OBJC_CLASS_\$_NSDate
l_date:
  .quad _OBJC_CLASS_\$_NSDate
.subsections_via_symbols
EOF

classrefs() { dyld_info -fixups $1 | grep '__objc_classrefs' | awk '{print $4, $5}' | sed 's|/| |'; }

$CC --ld-path=$mold -o $t/exe1 $t/a.o $t/b.o -framework Foundation -mmacosx-version-min=15.0
$RUN $t/exe1 | grep -q '^Foo 1$'
classrefs $t/exe1 > $t/refs1
grep -q '^bind [^ ]* _OBJC_CLASS_\$_NSDate$' $t/refs1
[ "$(grep -c . $t/refs1)" = 1 ]
dyld_info -fixups $t/exe1 > $t/fixups1
not grep -q '__got.*NSDate' $t/fixups1

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -framework Foundation -mmacosx-version-min=15.0 \
  -Wl,-dead_strip
$RUN $t/exe2 | grep -q '^Foo 1$'
otool -l $t/exe2 > $t/lc2
not grep -q __objc_classrefs $t/lc2

$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/b.o -framework Foundation -mmacosx-version-min=14.0
$RUN $t/exe3 | grep -q '^Foo 1$'
classrefs $t/exe3 > $t/refs3
[ "$(grep -c . $t/refs3)" = 2 ]
grep -q '^bind [^ ]* _OBJC_CLASS_\$_NSDate$' $t/refs3

$CC --ld-path=$mold -o $t/exe4 $t/a.o $t/b.o -framework Foundation -mmacosx-version-min=14.0 \
  -Wl,-dead_strip
$RUN $t/exe4 | grep -q '^Foo 1$'
classrefs $t/exe4 > $t/refs4
[ "$(grep -c . $t/refs4)" = 1 ]
not grep -q NSDate $t/refs4
