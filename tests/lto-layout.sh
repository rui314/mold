#!/bin/bash
source "$(dirname "$0")"/common.inc

# What LTO compiles, from a merged module or ThinLTO, is laid out with
# the native objects' code, a static function of the same name from
# each of two bitcode files included.
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
$RUN $t/exe
nm -m $t/exe | awk '$2 == "(__TEXT,__text)" && $NF ~ /^_(main|[bn][12]|st)$/ {print $NF}' \
  | sort > $t/syms
printf '_b1\n_b2\n_main\n_n1\n_n2\n_st\n_st\n' | diff - $t/syms

# A ThinLTO module's exception tables reach the unwinder: a C++
# exception thrown and caught in it works.
cat <<EOF | $CXX -O2 -flto=thin -c -xc++ - -o $t/c.o
#include <stdexcept>
struct Base { virtual ~Base(); };
Base::~Base() {}
__attribute__((noinline)) int f(int x) {
  try { if (x) throw std::runtime_error("x"); } catch (...) { return 1; }
  return 0;
}
int main(int argc, char **argv) { return f(argc) == 1 ? 0 : 1; }
EOF
$CXX --ld-path=$mold -o $t/c $t/c.o
otool -l $t/c | grep '^  sectname' > $t/sects
grep -q __gcc_except_tab $t/sects
$RUN $t/c
