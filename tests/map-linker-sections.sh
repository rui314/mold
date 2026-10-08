#!/bin/bash
source "$(dirname "$0")"/common.inc

# What the linker makes is file 0's in -map: a section it makes whole
# (the stubs, the GOT, the unwind info, __eh_frame, a -sectcreate
# section, ...) is one row of the section's address and size, named
# after it, and so is what it adds to a section past the input
# subsections there (the selector references of the Objective-C
# stubs). (ld-prime lists such content entry by entry, and credits a
# stub or a GOT slot to the file defining its symbol.)
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("hi\n"); return 0; }
EOF

# .cfi_escape (DW_CFA_GNU_args_size) leaves a function to DWARF unwind
# info.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl _dw
.p2align 2
_dw:
  .cfi_startproc
  .cfi_escape 0x2e, 0x10
  ret
  .cfi_endproc
EOF

cat <<EOF | $CC -o $t/c.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
- (int)bar;
@end
@implementation Foo
- (int)bar { return 1; }
@end
int call_bar(Foo *foo) { return [foo bar]; }
EOF

printf 'sixteen bytes!!\n' > $t/blob

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o -framework Foundation \
  -Wl,-sectcreate,__TEXT,__blob,$t/blob -Wl,-map,$t/map
$RUN $t/exe | grep -q hi

# The map's row for section $1,$2 that the linker made whole.
whole() {
  local sect=$(grep $'\t'"$1"$'\t'"$2"'$' $t/map | cut -f1,2)
  [ -n "$sect" ] && grep -qx "$sect"$'\t\\[  0\\] '"$1,$2" $t/map
}
whole __TEXT __stubs
whole __TEXT __unwind_info
whole __TEXT __eh_frame
whole __TEXT __blob
whole __DATA_CONST __got

# On arm64, the Objective-C stubs and the method lists in the relative
# form, and the stubs' selector references after the inputs'.
if [ $ARCH = arm64 ]; then
  whole __TEXT __objc_stubs
  whole __TEXT __objc_methlist
  grep -Eq $'^0x[0-9A-F]+\t0x[0-9A-F]+\t\\[  0\\] __DATA,__objc_selrefs$' $t/map
fi

# The symbols of the inputs stay their files'.
grep -Eq $'\t\\[  2\\] _dw$' $t/map
grep -Eq $'\t\\[  3\\] -\\[Foo bar\\]$' $t/map

# With lazy binding, the stub helper and the lazy pointers too. (A
# simulator's objects are built for its version, which binds no stub
# lazily.)
on_simulator && exit 0
cat <<EOF | $CC -o $t/d.o -c -xc - -mmacosx-version-min=11.0
#include <stdio.h>
int main() { puts("hi"); }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/d.o -mmacosx-version-min=11.0 -Wl,-map,$t/map
$RUN $t/exe2 | grep -q hi
whole __TEXT __stub_helper
whole __DATA __la_symbol_ptr
