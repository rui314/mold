#!/bin/bash
source "$(dirname "$0")"/common.inc

# .weak_def_can_be_hidden marks a weak definition whose address is
# never observed. ld64 demotes such a symbol to non-external in every
# kind of output, dylibs included, and calls it directly; a weak
# definition whose address escapes stays exported and coalescable,
# reached through a weak-lookup bind. We only auto-hid in executables.
cat <<EOF2 | $CXX -O2 -o $t/a.o -c -xc++ -
template <typename T> __attribute__((noinline)) T twice(T x) { return x + x; }
template <typename T> __attribute__((noinline)) T thrice(T x) { return x * 3; }
int (*tp)(int) = thrice<int>;
extern "C" int use(int x) { return twice(x) + thrice(x); }
EOF2
nm -m $t/a.o > $t/nm0
grep -q 'weak external automatically hidden __Z5twiceIiET_S0_' $t/nm0
grep -q 'weak external __Z6thriceIiET_S0_$' $t/nm0

$CXX --ld-path=$mold -dynamiclib -o $t/lib.dylib $t/a.o
nm -m $t/lib.dylib > $t/nm
grep -q 'non-external (was a private external) __Z5twiceIiET_S0_' $t/nm
grep -q 'weak external __Z6thriceIiET_S0_' $t/nm
dyld_info -exports $t/lib.dylib > $t/exports
not grep -q twice $t/exports
grep -q thrice $t/exports
# twice is called directly, thrice through a weak-lookup bind.
otool -Iv $t/lib.dylib | awk '/__stubs/{f=1;next} /Indirect/{f=0} f&&NF>=3{print $NF}' > $t/stubs
not grep -q twice $t/stubs
grep -q thrice $t/stubs
otool -hv $t/lib.dylib > $t/hdr
grep -q 'WEAK_DEFINES' $t/hdr

# With every weak definition hidden, the image defines none for dyld.
cat <<EOF2 | $CXX -O2 -o $t/b.o -c -xc++ -
template <typename T> __attribute__((noinline)) T twice(T x) { return x + x; }
extern "C" int use2(int x) { return twice(x); }
EOF2
$CXX --ld-path=$mold -dynamiclib -o $t/lib2.dylib $t/b.o
otool -hv $t/lib2.dylib > $t/hdr2
not grep -q 'WEAK_DEFINES' $t/hdr2
