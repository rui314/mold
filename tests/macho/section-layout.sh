#!/bin/bash
source "$(dirname "$0")"/common.inc

# Section order and alignment in a final image.
sections() { otool -l $1 | grep -E '^\s*sectname' | awk '{print $2}' | tr '\n' ' '; }
align() { otool -l $1 | grep -A8 "sectname $2\$" | grep align | head -1 | awk '{print $2}'; }

# Code leads each segment and zero fill closes it: the input's __bss,
# a C++ zero-initialized global's input __common and the synthesized
# __common of the common symbols all follow the file-backed data.
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
int cxx_var;
int main(int argc, char **) {
  try { printf("%d\n", g(argc) + total() + cxx_var); } catch (...) { puts("caught"); }
  return 0;
}
EOF
cat <<EOF | $CC -o $t/c.o -c -xc -
int g(int x) { if (x > 1) return x; return 0; }
EOF
cat <<EOF | $CC -o $t/f.o -c -xassembler -
.zerofill __DATA,__zz,_zz,8,3
EOF
$CXX --ld-path=$mold -o $t/exe $t/f.o $t/a.o $t/b.o $t/c.o
$RUN $t/exe | grep -Eq '^-?[0-9]+$'
sections $t/exe > $t/order
grep -Eq '__text .*__stubs .*__cstring .*__gcc_except_tab' $t/order
grep -Eq '__data (__bss|__common|__zz) (__bss|__common|__zz) (__bss|__common|__zz) ' $t/order

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

# A dylib with no code still has its function starts and export trie
# load commands. (ld-prime also writes an empty __text section.)
echo 'int data = 1;' | $CC -o $t/g.o -c -xc -
$CC --ld-path=$mold -dynamiclib -o $t/libdata.dylib $t/g.o -Wl,-exported_symbols_list,/dev/null
otool -l $t/libdata.dylib > $t/lc
not grep -q 'sectname __text' $t/lc
grep -q LC_FUNCTION_STARTS $t/lc
grep -q LC_DYLD_EXPORTS_TRIE $t/lc
