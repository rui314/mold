#!/bin/bash
source "$(dirname "$0")"/common.inc

# A weak definition that stays exported - not private, not auto-hidden
# - may be coalesced by dyld with another image's copy: whichever
# loads first wins, and every image must then use that one copy (C++'s
# one-definition rule: an inline function's static local is one
# variable, not one per dylib). ld64 routes every reference to such a
# symbol through a slot dyld binds by weak lookup - a GOT entry, a
# stub, a data pointer. We referenced our own copy directly, so two
# dylibs sharing an inline function each counted on their own static.
cat <<EOF2 > $t/counter.h
inline int next_id() { static int counter = 0; return ++counter; }
EOF2
for lib in a b; do
cat <<EOF2 | $CXX -O2 -I$t -o $t/$lib.o -c -xc++ -
#include "counter.h"
extern "C" int ${lib}_next() { return next_id(); }
EOF2
$CXX --ld-path=$mold -dynamiclib -o $t/lib$lib.dylib $t/$lib.o -install_name @rpath/lib$lib.dylib
done
cat <<EOF2 | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int a_next(void); int b_next(void);
int main() { printf("%d %d %d\n", a_next(), b_next(), a_next()); }
EOF2
$CC --ld-path=$mold -o $t/exe $t/main.o $t/liba.dylib $t/libb.dylib -Wl,-rpath,$t
# One counter across both dylibs.
$t/exe | grep -q '^1 2 3$'

# The static local's slot is bound by weak lookup; the header says so.
dyld_info -fixups $t/liba.dylib > $t/fixups
grep -q 'bind *<weak-def-coalesce>/__ZZ7next_idvE7counter' $t/fixups
otool -hv $t/liba.dylib | grep -q 'WEAK_DEFINES.*BINDS_TO_WEAK'

# A call to an exported weak function goes through a stub and a
# GOT slot bound the same way; a data pointer to it is bound too.
cat <<EOF2 | $CXX -O2 -o $t/w.o -c -xc++ -
template <typename T> __attribute__((noinline)) int wk(T x) { return x * 3; }
int (*wp)(int) = wk<int>;
extern "C" int call(int x) { return wk<int>(x); }
EOF2
$CXX --ld-path=$mold -dynamiclib -o $t/libw.dylib $t/w.o
dyld_info -fixups $t/libw.dylib > $t/wfixups
[ "$(grep -c 'bind *<weak-def-coalesce>/__Z2wkIiEiT_' $t/wfixups)" = 2 ]
otool -Iv $t/libw.dylib | awk '/__stubs/{f=1;next} /Indirect/{f=0} f&&NF>=3{print $NF}' | grep -q '^__Z2wkIiEiT_$'

# Classic dyld info (-undefined dynamic_lookup selects it): the slots
# are rebased to this image's copy and listed in the weak_bind stream.
for lib in a b; do
$CXX --ld-path=$mold -dynamiclib -o $t/lib${lib}c.dylib $t/$lib.o -install_name @rpath/lib${lib}c.dylib -Wl,-undefined,dynamic_lookup
done
otool -l $t/libac.dylib | grep -q 'LC_DYLD_INFO'
otool -l $t/libac.dylib | grep -A11 'LC_DYLD_INFO' | grep 'weak_bind_size' | grep -qv ' 0$'
$CC --ld-path=$mold -o $t/exec $t/main.o $t/libac.dylib $t/libbc.dylib -Wl,-rpath,$t
$t/exec | grep -q '^1 2 3$'
