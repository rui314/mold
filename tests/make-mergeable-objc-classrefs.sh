#!/bin/bash
source "$(dirname "$0")"/common.inc

# Class references fold into the GOT from iOS 18 (and macOS 15).
simulator_older_than 18 && skip

# From macOS 15 on, a class reference slot whose address code takes
# stays, as its class's GOT entry (see objc-classrefs-got-pairs.sh). A
# mergeable dylib records what refers to the slot as ld-prime does: as
# references to the GOT entry, an adrp and add of it as one, with no
# entry for the slot. Xcode 26's ld-prime merges no other record of it
# right, and mold merges ld-prime's.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc - -mmacosx-version-min=15.0
#import <Foundation/Foundation.h>
@interface Foo : NSObject
@end
@implementation Foo
@end
Class foo_class(void) { return [Foo class]; }
Class array_class(void) { return [NSMutableArray class]; }
EOF

if [ $ARCH = arm64 ]; then
  addr() { printf 'adrp x8, %s@PAGE\n  add x0, x8, %s@PAGEOFF\n  ret\n' $1 $1; }
else
  addr() { printf 'leaq %s(%%rip), %%rax\n  retq\n' $1; }
fi
cat <<EOF | $CC -o $t/b.o -c -xassembler - -mmacosx-version-min=15.0
.section __DATA,__objc_classrefs,regular,no_dead_strip
.p2align 3
Lfoo: .quad _OBJC_CLASS_\$_Foo
Larray: .quad _OBJC_CLASS_\$_NSMutableArray
.text
.globl _foo_slot, _array_slot
.p2align 2
_foo_slot:
  $(addr Lfoo)
_array_slot:
  $(addr Larray)
.data
.globl _foo_ptr
.p2align 3
_foo_ptr: .quad Lfoo
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/main.o -c -xc - -mmacosx-version-min=15.0
#include <stdio.h>
void *foo_class(void), *array_class(void), **foo_slot(void), **array_slot(void);
extern void **foo_ptr;
int main() {
  printf("%d %d %d\n", *foo_slot() == foo_class(), *array_slot() == array_class(),
         *foo_ptr == foo_class());
}
EOF

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o $t/b.o -framework Foundation \
  -Wl,-make_mergeable -Wl,-install_name,@rpath/libfoo.dylib -mmacosx-version-min=15.0
otool -l $t/libfoo.dylib > $t/lc
not grep -q __objc_classrefs $t/lc
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo -Wl,-no_merged_libraries_hook \
  -framework Foundation -mmacosx-version-min=15.0
$RUN $t/exe | grep -q '^1 1 1$'
$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo -Wl,-no_merged_libraries_hook \
  -framework Foundation -mmacosx-version-min=15.0
$RUN $t/exe2 | grep -q '^1 1 1$'

mkdir -p $t/ld
$CC -shared -o $t/ld/libfoo.dylib $t/a.o $t/b.o -framework Foundation \
  -Wl,-make_mergeable -Wl,-install_name,@rpath/libfoo.dylib -mmacosx-version-min=15.0
$CC --ld-path=$mold -o $t/exe3 $t/main.o -L$t/ld -Wl,-merge-lfoo -Wl,-no_merged_libraries_hook \
  -framework Foundation -mmacosx-version-min=15.0
$RUN $t/exe3 | grep -q '^1 1 1$'
