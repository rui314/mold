#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 orders the sections of a -r output by segment (__TEXT,
# __DATA_CONST, __DATA, others as first seen, __LD last) and, within
# __DATA, by a fixed rank table - the order a final link gives
# __DATA_CONST, then __DATA - with unknown sections in first-seen
# order and the zero-fill ones last. __TEXT keeps first-seen order
# after __text and the other code sections. We wrote sections in
# first-seen order across segments, interleaving __TEXT and __DATA.
cat <<EOF2 | $CC -o $t/a.o -c -xassembler -
.section __DATA,__const
.quad 1
.section __DATA,__bss
.zero 8
.section __DATA,__zz
.quad 1
.section __DATA_CONST,__const
.quad 1
.section __TEXT,__zz
.quad 1
.section __DATA,__mod_init_func,mod_init_funcs
.quad _fx
.section __DATA,__thread_data,thread_local_regular
.quad 1
.section __DATA,__thread_vars,thread_local_variables
.quad 0,0,0
.section __DATA,__thread_bss,thread_local_zerofill
.zero 8
.section __TEXT,__StaticInit,regular,pure_instructions
.p2align 2
_si: nop
.section __TEXT,__aa
.quad 1
.text
.globl _fx
.p2align 2
_fx: nop
EOF2
cat <<EOF2 | $CC -O2 -fobjc-arc -fno-asynchronous-unwind-tables -fno-exceptions -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@protocol P - (void)pm; @end
@interface Foo : NSObject { int _iv; } @property int p; @end
@implementation Foo + (void)load {} @end
@interface NSObject (Ext) - (int)ext; @end
@implementation NSObject (Ext) + (void)load {} - (int)ext { return 1; } @end
@interface Bar : NSObject @end
@implementation Bar - (NSString *)description { return [super description]; } @end
Protocol *getp(void) { return @protocol(P); }
Class getcls(void) { return [NSObject class]; }
SEL getsel(void) { return @selector(count); }
NSString *cf(void) { return @"cfstr"; }
EOF2

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
# (x86-64 objects also carry __eh_frame, which closes __TEXT; it is
# left out of the comparison.)
otool -l $t/r.o | awk '/sectname/{s=$2} /segname/{if(s!=""){printf "%s,%s ", $2, s; s=""}}' | sed 's/__TEXT,__eh_frame //' > $t/order
grep -q '^__TEXT,__text __TEXT,__StaticInit __TEXT,__zz __TEXT,__aa __TEXT,__objc_classname __TEXT,__objc_methname __TEXT,__objc_methtype __TEXT,__cstring __DATA_CONST,__const __DATA,__mod_init_func __DATA,__const __DATA,__cfstring __DATA,__objc_classlist __DATA,__objc_nlclslist __DATA,__objc_catlist __DATA,__objc_nlcatlist __DATA,__objc_protolist __DATA,__objc_imageinfo __DATA,__objc_const __DATA,__objc_selrefs __DATA,__objc_protorefs __DATA,__objc_classrefs __DATA,__objc_superrefs __DATA,__objc_ivar __DATA,__objc_data __DATA,__zz __DATA,__thread_vars __DATA,__data __DATA,__thread_data __DATA,__thread_bss __DATA,__bss __LD,__compact_unwind $' $t/order

# A zero-fill section in the middle takes no file space: the sections
# after it have offsets that skip its address span, as in ld64's
# layout.
otool -l $t/r.o | awk '/sectname/{n=$2} /^ *addr/{a=$2} /^ *size/{s=$2} /^ *offset/{print n, a, s, $2}' > $t/layout
zf_addr=$(awk '$1=="__thread_bss"{print $2}' $t/layout)
cu=$(awk '$1=="__compact_unwind"{print $2, $4}' $t/layout)
seg_fileoff=$(otool -l $t/r.o | grep -m1 fileoff | awk '{print $2}')
python3 - "$zf_addr" $cu $seg_fileoff <<'EOF2'
import sys
zf, cu_addr, cu_off, fileoff = int(sys.argv[1],16), int(sys.argv[2],16), int(sys.argv[3]), int(sys.argv[4])
# The run __thread_bss..__bss (and padding) has addresses but no
# file bytes: __compact_unwind's offset skips exactly that span.
assert cu_off == fileoff + zf, (zf, cu_addr, cu_off, fileoff)
assert cu_addr > zf
EOF2
