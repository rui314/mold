#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 -r writes local symbols in address order and names some atoms
# itself, with one counter: each cstring literal is LC<n> with N_PEXT
# set ("was a private external" in nm -m), the records of __cfstring,
# __objc_selrefs and __objc_classrefs are l<nnn> with N_PEXT, and the
# entries of the __objc_*list sections l<nnn> without it. Their
# original labels vanish. Other assembler labels survive only when
# they name an atom of their own.
cat <<EOF2 | $CC -O2 -fobjc-arc -fno-asynchronous-unwind-tables -fno-exceptions -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject @end
@implementation Foo + (void)load {} - (int)m { return 1; } @end
@interface Bar : NSObject @end
@implementation Bar - (NSString *)description { return [super description]; } @end
Class getcls(void) { return [NSObject class]; }
SEL getsel(void) { return @selector(count); }
NSString *cf(void) { return @"cfstr"; }
static const char *p = "ptr-lit";
const char *f3(void) { return p; }
EOF2
$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -xp $t/r.o | awk '$2!="0f" && $2!="01" {print $2, $NF}' > $t/locals

# Cstring literals, N_PEXT|N_SECT, numbered in address order.
grep -q '^1e LC1$' $t/locals
[ "$(grep -c '^1e LC[0-9]*$' $t/locals)" -ge 6 ]
not grep -q 'l_.str\|l_OBJC_METH_VAR_NAME\|l_OBJC_CLASS_NAME' $t/locals
# Anonymous records: classlist, nlclslist without N_PEXT; cfstring,
# selrefs, classrefs with it. The counter continues from the LCs.
n=$(grep -c '^1e LC' $t/locals)
[ "$(grep -c '^0e l[0-9][0-9][0-9]$' $t/locals)" = 3 ]
[ "$(grep -c '^1e l[0-9][0-9][0-9]$' $t/locals)" = 4 ]
not grep -q 'l_OBJC_LABEL_CLASS\|l__unnamed_cfstring\|_OBJC_SELECTOR_REFERENCES_\|_OBJC_CLASSLIST_REFERENCES' $t/locals
first_l=$(grep -m1 -o 'l[0-9][0-9][0-9]$' $t/locals | tr -d l | sed 's/^0*//')
[ "$first_l" = $((n + 1)) ]
# The superclass reference keeps its own label; the ltmp aliases go.
grep -q '^0e l_OBJC_CLASSLIST_SUP_REFS_\$_$' $t/locals
not grep -q ltmp $t/locals
# Locals come in address order.
nm -xp $t/r.o | awk '$2=="0e" || $2=="1e" {print $1}' > $t/addrs
sort -c $t/addrs
# The relocations that referred to the literals now name the atoms.
otool -rv $t/r.o > $t/relocs
grep -q ' LC[0-9]' $t/relocs
not grep -q 'l_.str\|l_OBJC_METH_VAR_NAME' $t/relocs
# And the merged object still links and runs.
cat <<EOF2 | $CC -O2 -fobjc-arc -o $t/main.o -c -xobjective-c -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
const char *f3(void); NSString *cf(void); Class getcls(void); SEL getsel(void);
int main() { return (getcls() == [NSObject class] && sel_isEqual(getsel(), @selector(count)) && [cf() isEqualToString:@"cfstr"] && f3()[0] == 'p') ? 0 : 1; }
EOF2
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -framework Foundation
$t/exe
