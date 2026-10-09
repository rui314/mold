#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output has every section of its inputs, in the order their first
# members come, the linker's own (__compact_unwind) after them, the
# zero-fill sections - the thread-local ones too - taking no file
# space. (ld-prime orders them by segment and a rank table; a later
# link orders the sections by its own rules either way.)
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
# (x86-64 objects also carry __eh_frame; it is left out.)
otool -l $t/r.o | awk '/sectname/{s=$2} /segname/{if(s!=""){print $2 "," s; s=""}}' |
  grep -v __TEXT,__eh_frame > $t/order
[ "$(sort $t/order | tr '\n' ' ')" = '__DATA,__bss __DATA,__cfstring __DATA,__const __DATA,__data __DATA,__mod_init_func __DATA,__objc_catlist __DATA,__objc_classlist __DATA,__objc_classrefs __DATA,__objc_const __DATA,__objc_data __DATA,__objc_imageinfo __DATA,__objc_ivar __DATA,__objc_nlcatlist __DATA,__objc_nlclslist __DATA,__objc_protolist __DATA,__objc_protorefs __DATA,__objc_selrefs __DATA,__objc_superrefs __DATA,__thread_bss __DATA,__thread_data __DATA,__thread_vars __DATA,__zz __DATA_CONST,__const __LD,__compact_unwind __TEXT,__StaticInit __TEXT,__aa __TEXT,__cstring __TEXT,__objc_classname __TEXT,__objc_methname __TEXT,__objc_methtype __TEXT,__text __TEXT,__zz ' ]
[ "$(head -2 $t/order | tr "\n" " ")" = "__TEXT,__text __DATA,__const " ]
otool -l $t/r.o > $t/lc
grep -A4 'sectname __bss' $t/lc | grep -q 'offset 0$'
grep -A4 'sectname __thread_bss' $t/lc | grep -q 'offset 0$'

# A final link of such an output runs: the Objective-C classes are
# registered, the initializer runs, the thread-local and zero-fill
# variables start out right.
cat <<EOF2 | $CC -o $t/c.o -c -xc -
__thread int tv = 3;
__thread int tz;
int bss[4];
int inited;
__attribute__((constructor)) static void init(void) { inited = 1; }
int sum(void) { return tv + tz + bss[2] + inited; }
EOF2
$mold -r -arch $ARCH -o $t/r2.o $t/b.o $t/c.o
cat <<EOF2 | $CC -o $t/main.o -c -xobjective-c -
#import <Foundation/Foundation.h>
NSString *cf(void);
SEL getsel(void);
int sum(void);
int main() {
  printf("%s %s %d %d\n", [cf() UTF8String], sel_getName(getsel()),
         NSClassFromString(@"Foo") != nil, sum());
}
EOF2
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r2.o -framework Foundation
$RUN $t/exe | grep '^cfstr count 1 4$'
