#!/bin/bash
source "$(dirname "$0")"/common.inc

# Every symbol a unit with debug info has in the symbol table gets a
# debug note (stab) where it lies: an exception table's label
# (GCC_except_table<n> in __gcc_except_tab), an ivar offset
# (_OBJC_IVAR_$_... in __objc_ivar), and the symbols of the sections
# whose contents the linker takes apart into subsections of its own:
# literals, terminator pointers, the Objective-C lists and references.
# dsymutil ignores a note no DWARF describes. (ld-prime notes none of
# those.) A global with an assembler-local name, as Swift's weak
# private l_OBJC_PROTOCOL_SYMREF_$_* are, is listed nowhere.
cat <<EOF | $CXX -o $t/a.o -c -g -xc++ -
#include <stdexcept>
int risky(int x) { try { if (x) throw std::runtime_error("x"); } catch (...) { return 1; } return 0; }
EOF
cat <<EOF | $CC -o $t/b.o -c -g -xobjective-c -
#import <Foundation/Foundation.h>
@protocol P
- (void)m;
@end
@interface Foo : NSObject <P> { @public int x; }
@end
@implementation Foo
- (void)m {}
@end
int main(void) { Foo *f = [Foo new]; f->x = 3; return f->x - 3 + !@protocol(P); }
EOF

if [ $ARCH = arm64 ]; then ret=ret; else ret=retq; fi
cat <<EOF | $CC -o $t/c.o -c -g -xassembler -
  .text
  .globl _fin
_fin:
  $ret
  .globl _cstr, _mystr, _lit8, _ustr, _term, _kept
  .cstring
_cstr:
  .asciz "abc"
  .section __TEXT,__mystr,cstring_literals
_mystr:
  .asciz "def"
  .literal8
_lit8:
  .quad 5
  .section __TEXT,__ustring
_ustr:
  .short 0x41, 0
  .mod_term_func
  .p2align 3
_term:
  .quad _fin
  .const
_kept:
  .long 1
  .globl l_pext, l_ext
  .private_extern l_pext
  .weak_definition l_pext
l_pext:
  .quad 2
l_ext:
  .quad 3
.subsections_via_symbols
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o -framework Foundation
$t/exe
nm -m $t/exe > $t/nm
grep -q GCC_except_table $t/nm
grep -q '_OBJC_IVAR_$_Foo.x' $t/nm
nm -ap $t/exe > $t/stabs
grep -Eq ' FUN __Z5riskyi$' $t/stabs
grep -Eq ' STSYM GCC_except_table' $t/stabs
grep -Eq ' GSYM _OBJC_IVAR_\$_Foo.x$' $t/stabs
grep -Eq ' GSYM __OBJC_LABEL_PROTOCOL_\$_P$' $t/stabs
grep -Eq ' GSYM __OBJC_PROTOCOL_REFERENCE_\$_P$' $t/stabs
for s in _kept _cstr _mystr _lit8 _term; do
  grep -Eq " GSYM $s\$" $t/stabs
done
# The reader demotes a global labeling UTF-16 strings to a local.
grep -Eq ' STSYM _ustr$' $t/stabs
not grep -Eq ' (STSYM|GSYM) l_(pext|ext)$' $t/stabs
dsymutil -o $t/exe.dSYM $t/exe > $t/dsym.log 2>&1
not grep -qi warning $t/dsym.log
