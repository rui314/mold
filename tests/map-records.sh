#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime's map lists the __eh_frame records it writes, credited to
# the object each came from: a CIE as "CFI", an FDE as "FDE for: " and
# its function's name. It spells a C string's bytes as they are, and
# credits itself with the method lists it rewrites in the relative
# form, and with the selector names of the Objective-C stubs it makes.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
const char *s = "caf\xe9!";
int main() { printf("%s\n", s); return 0; }
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

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o -framework Foundation -Wl,-map,$t/map
grep -Eq $'^0x[0-9A-F]+\t0x[0-9A-F]+\t\\[  2\\] CFI$' $t/map
grep -Eq $'^0x[0-9A-F]+\t0x[0-9A-F]+\t\\[  2\\] FDE for: _dw$' $t/map
grep -q $'\\[  1\\] literal string: caf\xe9!$' $t/map

if [ $ARCH = arm64 ]; then
  grep -Fq $'\t[  0] __OBJC_$_INSTANCE_METHODS_Foo' $t/map
  grep -Fq $'\t[  0] literal string: bar' $t/map
fi
