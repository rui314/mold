#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# A section$start$ or section$end$ symbol naming a pointer section only
# the linker makes finds it where ld-prime puts its own: __auth_got,
# __weak_got and __weak_auth_got in __DATA_CONST, and in the shared
# region __la_symbol_ptr too. From macOS 15 on, the class references
# fold into __got, and __objc_classrefs - empty now - moves to
# __DATA_CONST with the other reference lists; before, it stays in
# __DATA with the references in it.
cat <<'EOF' | $CC -o $t/a.o -c -xc - -mmacosx-version-min=14.0
#include <stdio.h>
extern char auth_got __asm("section$start$__DATA$__auth_got");
extern char weak_got __asm("section$start$__DATA$__weak_got");
extern char refs_start __asm("section$start$__DATA$__objc_classrefs");
extern char refs_end __asm("section$end$__DATA$__objc_classrefs");
void *use_classes(void);
int main() {
  use_classes();
  printf("%p %p %ld\n", &auth_got, &weak_got, (long)(&refs_end - &refs_start));
}
EOF
cat <<'EOF' | $CC -o $t/b.o -c -xobjective-c - -mmacosx-version-min=14.0
#import <Foundation/Foundation.h>
@interface Foo : NSObject
@end
@implementation Foo
@end
void *use_classes(void) {
  return (__bridge void *)[NSString class] == (__bridge void *)[Foo class] ? 0 : (void *)1;
}
EOF

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { g = $2 }
    $1 == "size" && s != "" { z = $2 } $1 == "flags" && s != "" { print g "," s, z, $2; s = "" }'
}

$CC --ld-path=$mold -o $t/exe15 $t/a.o $t/b.o -framework Foundation -mmacosx-version-min=15.0
sects $t/exe15 > $t/sects15
grep -q '^__DATA_CONST,__auth_got 0x0*0 0x00000006$' $t/sects15
grep -q '^__DATA_CONST,__weak_got 0x0*0 0x00000006$' $t/sects15
grep -q '^__DATA_CONST,__objc_classrefs 0x0*0 ' $t/sects15
not grep -q '^__DATA,__objc_classrefs' $t/sects15
$RUN $t/exe15 > $t/out15
grep -q ' 0$' $t/out15

$CC --ld-path=$mold -o $t/exe14 $t/a.o $t/b.o -framework Foundation -mmacosx-version-min=14.0
sects $t/exe14 > $t/sects14
grep -q '^__DATA_CONST,__auth_got ' $t/sects14
grep -q '^__DATA,__objc_classrefs 0x0*10 ' $t/sects14
$RUN $t/exe14 > $t/out14
grep -q ' 16$' $t/out14

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -framework Foundation -Wl,-no_data_const
sects $t/exe2 > $t/sects2
grep -q '^__DATA,__auth_got ' $t/sects2
grep -q '^__DATA,__objc_classrefs ' $t/sects2
not grep -q __DATA_CONST $t/sects2

cat <<'EOF' | $CC -o $t/c.o -c -xc -
extern char start __asm("section$start$__DATA$__la_symbol_ptr");
void *get_start(void) { return &start; }
EOF
$CC --ld-path=$mold -o $t/libfoo.dylib $t/c.o -shared -Wl,-install_name,/usr/lib/libfoo.dylib
sects $t/libfoo.dylib > $t/sects3
grep -q '^__DATA_CONST,__la_symbol_ptr ' $t/sects3
$CC --ld-path=$mold -o $t/libbar.dylib $t/c.o -shared
sects $t/libbar.dylib > $t/sects4
grep -q '^__DATA,__la_symbol_ptr ' $t/sects4
