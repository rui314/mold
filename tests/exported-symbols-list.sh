#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int foo() { return 1; }
int bar() { return 2; }
int main() { return 0; }
EOF2

cat <<EOF2 > $t/list
# only foo is exported
_foo
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-exported_symbols_list,$t/list
dyld_info -exports $t/exe > $t/exports
grep -q _foo $t/exports
not grep -q _bar $t/exports

# ld-prime turns what the list leaves out into a private extern: a
# local in the symbol table, not only a name missing from the trie.
nm -m $t/exe > $t/syms
grep -q 'non-external (was a private external) _bar' $t/syms
grep -q 'non-external (was a private external) __mh_execute_header' $t/syms

cat <<EOF2 | $CC -o $t/b.o -c -xc -
__attribute__((weak)) int wf(void) { return 1; }
int g(void) { return wf(); }
int h(void) { return 2; }
EOF2

# A dylib's hidden definition is no dead-strip root.
$CC --ld-path=$mold -dynamiclib -o $t/libb.dylib $t/b.o \
  -Wl,-exported_symbol,_g,-dead_strip
nm $t/libb.dylib > $t/syms
not grep -q _h $t/syms

# A hidden weak definition is not coalesced: no weak-lookup bind, and
# neither WEAK_DEFINES nor BINDS_TO_WEAK.
$CC --ld-path=$mold -dynamiclib -o $t/libc.dylib $t/b.o \
  -Wl,-unexported_symbol,_wf
[ "$(otool -h $t/libc.dylib | tail -1 | awk '{print $NF}')" = 0x00100085 ]
dyld_info -fixups $t/libc.dylib > $t/fixups
not grep -q weak-def-coalesce $t/fixups

$mold -arch $ARCH -r $t/b.o -exported_symbol _g -o $t/c.o
nm -m $t/c.o > $t/syms
grep -q 'non-external (was a private external) _h' $t/syms
