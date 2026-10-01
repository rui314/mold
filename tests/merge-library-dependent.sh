#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib may link another, which the image merges as well:
# what the one imports from the other, the other's merged code then
# defines, and the image loads neither (the recorded dependency gets
# no load command, which dyld would fail to find).
cat <<EOF | $CC -o $t/b.o -c -O1 -xc -
int b_var = 40;
__thread int b_tlv = 2;
int b_func(int x) { return x + b_var; }
EOF
cat <<EOF | $CC -o $t/a.o -c -O1 -xc -
extern int b_var;
extern __thread int b_tlv;
int b_func(int);
int (*a_fp(void))(int) { return b_func; }
int a_func(int x) { b_tlv++; return b_var + b_tlv + a_fp()(x); }
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int a_func(int);
int main() { printf("%d\n", a_func(1)); }
EOF

mkdir -p $t/m $t/l
$CC --ld-path=$mold -shared -o $t/m/libb.dylib $t/b.o -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libb.dylib
$CC --ld-path=$mold -shared -o $t/m/liba.dylib $t/a.o -L$t/m -lb -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/liba.dylib
$CC -shared -o $t/l/libb.dylib $t/b.o -Wl,-make_mergeable -Wl,-install_name,@rpath/libb.dylib
$CC -shared -o $t/l/liba.dylib $t/a.o -L$t/l -lb -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/liba.dylib

for lib in m l; do
  $CC --ld-path=$mold -o $t/exe-$lib $t/main.o -L$t/$lib -Wl,-merge-la -Wl,-merge-lb
  $t/exe-$lib | grep -q '^84$'
  otool -L $t/exe-$lib > $t/libs-$lib
  not grep -q 'lib[ab]\.dylib' $t/libs-$lib
done

$CC -o $t/exe2 $t/main.o -L$t/m -Wl,-merge-la -Wl,-merge-lb
$t/exe2 | grep -q '^84$'
