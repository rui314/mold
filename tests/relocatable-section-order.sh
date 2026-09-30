#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime orders the sections of a -r output by segment - __TEXT
# first, __LD last, the others (__DATA_CONST too) as first seen - and,
# within __DATA, by a fixed rank table: __const, the Objective-C
# sections, the initializer lists, __data, then unknown sections in
# first-seen order and the thread-local template and zero-fill ones
# last. __TEXT has the code sections first (__StaticInit after the
# others), then the rest in first-seen order.
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
grep -q '^__TEXT,__text __TEXT,__StaticInit __TEXT,__zz __TEXT,__aa __TEXT,__objc_classname __TEXT,__objc_methname __TEXT,__objc_methtype __TEXT,__cstring __DATA,__const __DATA,__cfstring __DATA,__objc_classlist __DATA,__objc_nlclslist __DATA,__objc_catlist __DATA,__objc_nlcatlist __DATA,__objc_protolist __DATA,__objc_imageinfo __DATA,__objc_const __DATA,__objc_selrefs __DATA,__objc_protorefs __DATA,__objc_classrefs __DATA,__objc_superrefs __DATA,__objc_ivar __DATA,__objc_data __DATA,__mod_init_func __DATA,__data __DATA,__zz __DATA,__thread_vars __DATA,__thread_data __DATA,__thread_bss __DATA,__bss __DATA_CONST,__const __LD,__compact_unwind $' $t/order

# A zero-fill section in the middle takes no file space: the sections
# after it have offsets that skip its address span, as in ld64's
# layout.
otool -l $t/r.o | awk '/sectname/{n=$2} /^ *addr/{a=$2} /^ *size/{s=$2} /^ *offset/{print n, a, s, $2}' > $t/layout
seg_fileoff=$(otool -l $t/r.o | grep -m1 fileoff | awk '{print $2}')
python3 - $t/layout $seg_fileoff <<'EOF2'
import sys
rows = [l.split() for l in open(sys.argv[1])]
fileoff = int(sys.argv[2])
i = [r[0] for r in rows].index('__thread_bss')
zf = int(rows[i][1], 16)
# The run __thread_bss..__bss (and padding) has addresses but no
# file bytes: the next section's offset skips exactly that span.
nxt = next(r for r in rows[i:] if int(r[3]) != 0)
assert int(nxt[3]) == fileoff + zf, (nxt, zf, fileoff)
assert int(nxt[1], 16) > zf
EOF2

# Code sections, __text included, keep first-seen order, and so do the
# segments after __TEXT: here a custom code section and __DATA_CONST
# come first.
cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.section __DATA_CONST,__cc
.quad 1
.section __TEXT,__zcode,regular,pure_instructions
_zc: ret
.section __DATA,__dd
.quad 1
.subsections_via_symbols
EOF2
cat <<EOF2 | $CC -o $t/d.o -c -xassembler -
.text
_tx: ret
.subsections_via_symbols
EOF2
$mold -r -arch $ARCH -o $t/r2.o $t/c.o $t/d.o
otool -l $t/r2.o | awk '/sectname/{s=$2} /segname/{if(s!=""){printf "%s,%s ", $2, s; s=""}}' | sed 's/__TEXT,__eh_frame //' > $t/order2
grep -q '^__TEXT,__zcode __TEXT,__text __DATA_CONST,__cc __DATA,__dd ' $t/order2
