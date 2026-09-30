#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime writes no debug note (stab) for an exception table's label
# (GCC_except_table<n> in __gcc_except_tab) or an ivar offset
# (_OBJC_IVAR_$_... in __objc_ivar), though both stay in the symbol
# table; a function or other data in the unit keeps its note.
cat <<EOF | $CXX -o $t/a.o -c -g -xc++ -
#include <stdexcept>
int risky(int x) { try { if (x) throw std::runtime_error("x"); } catch (...) { return 1; } return 0; }
EOF
cat <<EOF | $CC -o $t/b.o -c -g -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject { @public int x; }
@end
@implementation Foo
@end
int main(void) { Foo *f = [Foo new]; f->x = 3; return f->x - 3; }
EOF
$CXX --ld-path=$mold -o $t/exe $t/a.o $t/b.o -framework Foundation
$t/exe
nm -m $t/exe > $t/nm
grep -q GCC_except_table $t/nm
grep -q '_OBJC_IVAR_$_Foo.x' $t/nm
nm -ap $t/exe > $t/stabs
grep -Eq ' FUN __Z5riskyi$' $t/stabs
not grep -Eq ' (STSYM|GSYM) (GCC_except_table|_OBJC_IVAR_)' $t/stabs
