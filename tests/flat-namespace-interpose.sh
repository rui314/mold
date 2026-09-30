#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -flat_namespace dylib or bundle binds its own references to what it
# exports by flat lookup, as it binds its imports, so that an image
# loaded before it - here the main executable - can interpose them:
# calls go through stubs, GOT loads stay, and pointers in data are
# binds, not rebases. Private externs and statics are rebased; an
# executable's own references are not affected.
cat <<EOF | $CC -o $t/a.o -c -xc - -mmacosx-version-min=11.0
int gvar = 1;
static int svar = 2;
__attribute__((visibility("hidden"))) int hvar = 3;
int gfunc(void) { return 1; }
int *gptr = &gvar;
int *sptr = &svar;
int *hptr = &hvar;
int (*fptr)(void) = gfunc;
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -mmacosx-version-min=11.0
extern int gvar, *gptr, *sptr, *hptr;
int gfunc(void);
extern int (*fptr)(void);
int get(void) { return gvar * 1000 + *gptr * 100 + gfunc() * 10 + fptr() + *sptr + *hptr; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int gvar = 4;
int gfunc(void) { return 5; }
int get(void);
int main() { printf("%d\n", get()); }
EOF

# Classic dyld info with lazy binding, then chained fixups.
$CC --ld-path=$mold -o $t/libfoo.dylib -shared $t/a.o $t/b.o \
  -Wl,-flat_namespace -mmacosx-version-min=11.0
$CC --ld-path=$mold -o $t/exe $t/c.o $t/libfoo.dylib
$t/exe | grep -q '^4460$'

$CC --ld-path=$mold -o $t/libfoo.dylib -shared $t/a.o $t/b.o \
  -Wl,-flat_namespace -mmacosx-version-min=13.0
$t/exe | grep -q '^4460$'
dyld_info -fixups $t/libfoo.dylib > $t/fixups
grep -Eq '__data .* bind +<flat-namespace>/_gvar$' $t/fixups
grep -Eq '__data .* bind +<flat-namespace>/_gfunc$' $t/fixups
grep -Eq '__got .* bind +<flat-namespace>/_gvar$' $t/fixups
grep -Eq '__got .* bind +<flat-namespace>/_gfunc$' $t/fixups
[ "$(grep -c '__data .* rebase ' $t/fixups)" = 2 ]

# A two-level namespace dylib keeps them to itself.
$CC --ld-path=$mold -o $t/libfoo.dylib -shared $t/a.o $t/b.o
$t/exe | grep -q '^1116$'
