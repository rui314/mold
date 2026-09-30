#!/bin/bash
source "$(dirname "$0")"/common.inc

# Section order and alignment as ld-prime lays a final image out.
sections() { otool -l $1 | grep -E '^\s*sectname' | awk '{print $2}' | tr '\n' ' '; }
align() { otool -l $1 | grep -A8 "sectname $2\$" | grep align | head -1 | awk '{print $2}'; }

# Equally ranked input sections keep their first-seen order (object,
# then section): a.o's __cstring precedes b.o's __gcc_except_tab, its
# __bss precedes the synthesized __common of its own common symbol,
# and __DATA_CONST,__const precedes the GOT, which closes the segment.
cat <<EOF | $CC -o $t/a.o -c -xc - -fcommon
#include <stdio.h>
int data_var = 3;
int common_var;
static int bss_arr[100];
int *const ptrs[] = {&data_var, bss_arr};
void hello(void) { puts("hello"); }
int total(void) { return data_var + common_var + bss_arr[1] + (int)(ptrs[0] - ptrs[1]); }
EOF
cat <<EOF | $CXX -o $t/b.o -c -xc++ -
#include <cstdio>
extern "C" int total(void);
extern "C" int g(int);
int main(int argc, char **) {
  try { printf("%d\n", g(argc) + total()); } catch (...) { puts("caught"); }
  return 0;
}
EOF
cat <<EOF | $CC -o $t/c.o -c -xc -
int g(int x) { if (x > 1) return x; return 0; }
EOF
$CXX --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep -Eq '^-?[0-9]+$'
sections $t/exe > $t/order
grep -Eq '__cstring .*__gcc_except_tab' $t/order
grep -Eq '__const .*__got .*__data' $t/order
grep -Eq '__data __bss __common' $t/order

# With the common symbol claimed by an earlier object than the one
# with __bss, __common comes first.
cat <<EOF | $CC -o $t/d.o -c -xc - -fcommon
int common_var;
EOF
$CXX --ld-path=$mold -o $t/exe2 $t/d.o $t/a.o $t/b.o $t/c.o
sections $t/exe2 > $t/order2
grep -Eq '__data __common __bss' $t/order2

# A common symbol without an alignment of its own is aligned to its
# size rounded up to a power of two: up to the page on arm64, 16
# bytes on x86-64.
cat <<EOF | $CC -o $t/e.o -c -xc - -fcommon
char big[100000];
char small[100];
int main() { return big[0] + small[0]; }
EOF
$CC --ld-path=$mold -o $t/exe3 $t/e.o
if [ $ARCH = arm64 ]; then
  [ "$(align $t/exe3 __common)" = 2^14 ]
else
  [ "$(align $t/exe3 __common)" = 2^4 ]
fi
nm -n $t/exe3 | grep '_small$'

# The thread-local template's two sections get the stricter alignment.
cat <<EOF | $CC -o $t/f.o -c -xc -
_Thread_local int tdata = 1;
_Thread_local long long tbss[4] __attribute__((aligned(16)));
int main() { return tdata + (int)tbss[0]; }
EOF
$CC --ld-path=$mold -o $t/exe4 $t/f.o
[ "$(align $t/exe4 __thread_data)" = 2^4 ]
[ "$(align $t/exe4 __thread_bss)" = 2^4 ]

# A dylib with no code still has an (empty) __text section, and its
# function starts and export trie load commands.
echo 'int data = 1;' | $CC -o $t/g.o -c -xc -
$CC --ld-path=$mold -dynamiclib -o $t/libdata.dylib $t/g.o -Wl,-exported_symbols_list,/dev/null
otool -l $t/libdata.dylib > $t/lc
grep -q 'sectname __text' $t/lc
grep -q LC_FUNCTION_STARTS $t/lc
grep -q LC_DYLD_EXPORTS_TRIE $t/lc
