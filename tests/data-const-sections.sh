#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime moves the sections on its list from __DATA to __DATA_CONST:
# among them __auth_ptr, the signed pointers clang emits for arm64e,
# and __objc_catlist2, the category list of classes with stubs, which
# stays no-dead-strip like the other lists. A section not on the list,
# such as __objc_boolobj, stays in __DATA, and -no_data_const keeps
# them all there.
cat <<EOF | $CC -o $t/a.o -c -xc -
#define SECT(s) __attribute__((used, section("__DATA," s)))
SECT("__auth_ptr") void *auth_ptr = &auth_ptr;
SECT("__objc_dateobj") void *dateobj = &dateobj;
SECT("__const_cfobj2") void *cfobj2 = &cfobj2;
SECT("__objc_boolobj") void *boolobj = &boolobj;
int main() { return auth_ptr != &auth_ptr || cfobj2 != &cfobj2; }
EOF

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { g = $2 }
    $1 == "flags" && s != "" { print g "," s, $2; s = "" }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o
sects $t/exe > $t/sects
grep -q '^__DATA_CONST,__auth_ptr ' $t/sects
grep -q '^__DATA_CONST,__objc_dateobj ' $t/sects
grep -q '^__DATA_CONST,__const_cfobj2 ' $t/sects
grep -q '^__DATA,__objc_boolobj ' $t/sects
$t/exe

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_data_const
sects $t/exe2 > $t/sects2
not grep -q __DATA_CONST $t/sects2
grep -q '^__DATA,__auth_ptr ' $t/sects2
$t/exe2

cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
__attribute__((objc_class_stub)) __attribute__((objc_subclassing_restricted))
@interface Stub
@end
@interface Stub (Cat)
- (void)m;
@end
@implementation Stub (Cat)
- (void)m {}
@end
int main() { return 0; }
EOF

$CC --ld-path=$mold -o $t/exe3 $t/b.o -Wl,-undefined,dynamic_lookup
sects $t/exe3 > $t/sects3
grep -q '^__DATA_CONST,__objc_catlist2 0x00000000$' $t/sects3

# ld-prime knows the initializer and terminator lists by their types,
# as it does non-lazy pointers: a __DATA section of either type moves
# to __DATA_CONST whatever its name (a regular __mod_term_func stays),
# and -rename_section names it there.
cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.section __DATA,__myterm,mod_term_funcs
.p2align 3
.quad _main
.section __DATA,__myinit,mod_init_funcs
.p2align 3
.quad _main
.section __DATA,__mod_term_func
.p2align 3
.quad 1
.text
.globl _main
_main: ret
.subsections_via_symbols
EOF2
$CC --ld-path=$mold -o $t/exe4 $t/c.o -Wl,-no_fixup_chains \
  -Wl,-rename_section,__DATA_CONST,__myinit,__X,__init
sects $t/exe4 > $t/sects4
grep -q '^__DATA_CONST,__myterm 0x0000000a$' $t/sects4
grep -q '^__X,__init 0x00000009$' $t/sects4
grep -q '^__DATA,__mod_term_func 0x00000000$' $t/sects4
$CC --ld-path=$mold -o $t/exe5 $t/c.o -Wl,-no_fixup_chains -Wl,-no_data_const
sects $t/exe5 > $t/sects5
grep -q '^__DATA,__myterm 0x0000000a$' $t/sects5
grep -q '^__DATA,__myinit 0x00000009$' $t/sects5
