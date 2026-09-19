#!/bin/bash
source "$(dirname "$0")"/common.inc

# libc++ exports operator new and delete as weak definitions so that a
# program may override them. ld64 binds a reference to a dylib's weak
# export by weak lookup - dyld searches every loaded image for the
# coalesced definition - not to that one dylib: with chained fixups
# the import's library ordinal is -3, with classic dyld info the slot
# is bound to the dylib and listed in the weak_bind stream as well,
# and the call never binds lazily. We bound them like any import.
cat <<EOF2 | $CXX -O2 -o $t/a.o -c -xc++ -
#include <new>
int *p;
extern "C" int f(int n) { p = new int[n]; p[0] = n; int v = p[0]; delete[] p; return v; }
EOF2
$CXX --ld-path=$mold -dynamiclib -o $t/lib.dylib $t/a.o
dyld_info -fixups $t/lib.dylib > $t/fixups
grep -q 'bind *<weak-def-coalesce>/__Znam' $t/fixups
grep -q 'bind *<weak-def-coalesce>/__ZdaPv' $t/fixups
otool -hv $t/lib.dylib | grep -q 'BINDS_TO_WEAK'

# Classic dyld info: bound to libc++ (the definition dyld starts from)
# and listed in weak_bind; no lazy binding for these calls.
$CXX --ld-path=$mold -dynamiclib -o $t/libc.dylib $t/a.o -Wl,-undefined,dynamic_lookup
dyld_info -fixups $t/libc.dylib > $t/cfixups
grep -q 'bind *libc++/__Znam' $t/cfixups
otool -l $t/libc.dylib | grep -A11 'LC_DYLD_INFO' > $t/info
grep 'weak_bind_size' $t/info | grep -qv ' 0$'
grep 'lazy_bind_size' $t/info | grep -q ' 0$'

# And an override in the executable wins for both dylibs.
cat <<EOF2 | $CXX -O2 -o $t/main.o -c -xc++ -
#include <cstdio>
#include <cstdlib>
#include <new>
static int news = 0;
void *operator new[](std::size_t n) { news++; return std::malloc(n); }
void operator delete[](void *p) noexcept { std::free(p); }
extern "C" int f(int);
int main() { int v = f(7); printf("%d %d\n", v, news); }
EOF2
$CXX --ld-path=$mold -o $t/exe $t/main.o $t/lib.dylib -Wl,-rpath,$t
$t/exe | grep -q '^7 1$'
$CXX --ld-path=$mold -o $t/exec $t/main.o $t/libc.dylib -Wl,-rpath,$t
$t/exec | grep -q '^7 1$'
