#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime lays out what LTO compiled where the bitcode files it is
# credited to were named, among the other objects' code, whether one
# merged module or ThinLTO compiled it; what it can't credit to one
# file (a static function two of them define) goes after every input.
echo 'int n1(void); int n2(void); int b1(void); int b2(void);
int main() { return n1() + n2() + b1() + b2() == 10 ? 0 : 1; }' | $CC -O2 -c -xc - -o $t/m.o
echo 'int n1(void) { return 1; }' | $CC -O2 -c -xc - -o $t/n1.o
echo 'int n2(void) { return 2; }' | $CC -O2 -c -xc - -o $t/n2.o
echo 'static volatile int v = 3;
__attribute__((noinline)) static int st(void) { return v; }
__attribute__((noinline)) int b1(void) { return st(); }' | $CC -O2 -flto -c -xc - -o $t/b1.o
echo 'static volatile int v = 4;
__attribute__((noinline)) static int st(void) { return v; }
__attribute__((noinline)) int b2(void) { return st(); }' | $CC -O2 -flto=thin -c -xc - -o $t/b2.o

$CC --ld-path=$mold -o $t/exe $t/m.o $t/b1.o $t/n1.o $t/b2.o $t/n2.o
$t/exe
nm -n $t/exe | awk '$3 ~ /^_(main|[bn][12]|st)$/ {print $3}' > $t/order
printf '_main\n_b1\n_n1\n_b2\n_n2\n_st\n_st\n' | diff - $t/order

# The output sections come in the order of their first atoms so laid
# out: a ThinLTO module's exception tables have no names and stay its
# object's, after a class's type name credited to the bitcode file.
cat <<EOF | $CXX -O2 -flto=thin -c -xc++ - -o $t/c.o
#include <stdexcept>
struct Base { virtual ~Base(); };
Base::~Base() {}
int f(int x) {
  try { if (x) throw std::runtime_error("x"); } catch (...) { return 1; }
  return 0;
}
EOF
$CXX --ld-path=$mold -shared -o $t/c.dylib $t/c.o
otool -l $t/c.dylib | grep '^  sectname' > $t/sects
grep -A1 ' __const$' $t/sects | grep -q __gcc_except_tab
