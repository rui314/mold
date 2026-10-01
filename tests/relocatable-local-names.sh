#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 -r writes each object's local symbols in the order of its
# sections, and on arm64 names some subsections itself with one counter
# running in that order: each cstring literal is LC<n> with N_PEXT
# set ("was a private external" in nm -m), and the records of
# __cfstring, __objc_selrefs and __objc_classrefs are l<nnn> with
# N_PEXT. Their original labels vanish, as do those of the entries of
# the __objc_*list sections, which get no symbol at all (ld-prime).
# x86-64 relocations refer to those literals and records
# section-relatively instead, and only the literals of __TEXT,__cstring
# get names (LC<n>). Other assembler labels survive only when they name
# a subsection of their own.
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

# Cstring literals, N_PEXT|N_SECT.
grep -q '^1e LC1$' $t/locals
not grep -q 'l_.str\|l_OBJC_METH_VAR_NAME\|l_OBJC_CLASS_NAME' $t/locals
# Anonymous records: cfstring, selrefs, classrefs with N_PEXT on
# arm64; the classlist and nlclslist entries carry no symbol.
[ "$(grep -c '^0e l[0-9][0-9][0-9]$' $t/locals)" = 0 ]
not grep -q 'l_OBJC_LABEL_CLASS\|l__unnamed_cfstring\|_OBJC_SELECTOR_REFERENCES_\|_OBJC_CLASSLIST_REFERENCES' $t/locals
# One counter numbers them in the order they are listed: the object's
# method names and types, then its selector and class references, its
# cstring literals and its cfstring (LC1-9, l010-l012, LC13-14, l015).
awk '$2 ~ /^(LC[0-9]+|l[0-9][0-9][0-9])$/ { sub(/^(LC|l)/, "", $2); print $2 + 0 }' $t/locals > $t/nums
[ "$(tr '\n' ' ' < $t/nums)" = "$(seq 1 $(wc -l < $t/nums) | tr '\n' ' ')" ]
if [ "$ARCH" = arm64 ]; then
  [ "$(grep -c '^1e LC[0-9]*$' $t/locals)" -ge 6 ]
  [ "$(grep -c '^1e l[0-9][0-9][0-9]$' $t/locals)" = 4 ]
  grep -A3 '^1e LC9$' $t/locals | grep -q '__OBJC_$_CLASS_METHODS_Foo'
  # The superclass reference keeps its own label.
  grep -q '^0e l_OBJC_CLASSLIST_SUP_REFS_\$_$' $t/locals
else
  # "cfstr" and "ptr-lit" alone, and no linker-private label of a
  # superclass reference.
  [ "$(grep -c '^1e LC[0-9]*$' $t/locals)" = 2 ]
  [ "$(grep -c '^1e l[0-9][0-9][0-9]$' $t/locals)" = 0 ]
  not grep -q l_OBJC_CLASSLIST_SUP_REFS_ $t/locals
fi
# The ltmp aliases go.
not grep -q ltmp $t/locals
# The relocations that referred to the literals now name them (LC<n>).
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
