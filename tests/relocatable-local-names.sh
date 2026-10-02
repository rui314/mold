#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output lists an object's local symbols as the object does - the
# linker-private (l) and temporary (L) labels the assembler kept
# included - and makes up none, and its relocations keep naming them.
# (ld-prime merges the literals and names them itself, LC<n> and
# l<nnn>, and drops the labels that named them.) Only the labels of
# the sections the output makes anew, __objc_imageinfo and
# __LD,__compact_unwind, go with the input sections.
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
locals() {
  nm -m $1 | grep ' non-external ' | grep -v '__objc_imageinfo\|__compact_unwind' |
    cut -c18- | sed 's/\[no dead strip\] //' | sort
}
locals $t/a.o > $t/locals-a
locals $t/r.o > $t/locals-r
diff $t/locals-a $t/locals-r
otool -rv $t/a.o | awk '$5 == "True" {print $NF}' | sort > $t/relocs-a
otool -rv $t/r.o | awk '$5 == "True" {print $NF}' | sort > $t/relocs-r
diff $t/relocs-a $t/relocs-r
# And the merged object still links and runs.
cat <<EOF2 | $CC -O2 -fobjc-arc -o $t/main.o -c -xobjective-c -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
const char *f3(void); NSString *cf(void); Class getcls(void); SEL getsel(void);
int main() { return (getcls() == [NSObject class] && sel_isEqual(getsel(), @selector(count)) && [cf() isEqualToString:@"cfstr"] && f3()[0] == 'p') ? 0 : 1; }
EOF2
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -framework Foundation
$t/exe
$CC -o $t/exe2 $t/main.o $t/r.o -framework Foundation
$t/exe2
