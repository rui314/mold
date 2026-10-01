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

# Nor does it note a symbol in a section whose contents it takes apart
# into subsections of its own: literals - by the section type, as for C
# strings in any section - and UTF-16 strings in __TEXT,__ustring,
# terminator pointers, and the Objective-C lists and references, such
# as the weak private externals clang names a protocol's entries in
# __objc_protolist and __objc_protorefs by (__OBJC_LABEL_PROTOCOL_$_P,
# __OBJC_PROTOCOL_REFERENCE_$_P). Nor a global with an assembler-local
# name, as Swift's weak private l_OBJC_PROTOCOL_SYMREF_$_* are.
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
grep -Eq ' GSYM _kept$' $t/stabs
not grep -Eq ' (STSYM|GSYM) (GCC_except_table|_OBJC_IVAR_)' $t/stabs
not grep -Eq ' (STSYM|GSYM) __OBJC_(LABEL_PROTOCOL|PROTOCOL_REFERENCE)_' $t/stabs
not grep -Eq ' (STSYM|GSYM) _(cstr|mystr|lit8|ustr|term)$' $t/stabs
not grep -Eq ' (STSYM|GSYM) l_(pext|ext)$' $t/stabs
