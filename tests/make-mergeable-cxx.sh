#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib's C++ subsections keep their exceptions (the LSDA,
# with its type information through GOT slots, the personality and the
# unwind records), vtables and weak definitions, so that the code runs
# merged by either linker.
cat <<EOF | $CXX -o $t/a.o -c -O1 -xc++ -
#include <stdexcept>
#include <string>
#include <vector>
#include <typeinfo>
struct Base { virtual ~Base() {} virtual int f() const { return 1; } };
struct Derived : Base { int v; Derived(int v) : v(v) {} int f() const override { return v; } };
inline int inl(int x) { static int n; return x + ++n; }
static std::string g_str = "global string";
int may_throw(int x) {
  if (x > 3) throw std::runtime_error("too big");
  return x;
}
extern "C" int cxx_entry(int x) {
  std::vector<Base *> v;
  v.push_back(new Derived(x));
  v.push_back(new Base());
  int sum = 0;
  for (auto *b : v) { sum += b->f() + (int)std::string(typeid(*b).name()).size(); delete b; }
  try { sum += may_throw(x); } catch (const std::exception &e) { sum += 100; }
  return sum + inl(x) + (int)g_str.size();
}
EOF

cat <<EOF | $CXX -o $t/main.o -c -O1 -xc++ -
#include <cstdio>
inline int inl(int x) { static int n; return x + ++n; }
extern "C" int cxx_entry(int);
int main() { printf("%d %d %d\n", cxx_entry(2), cxx_entry(5), inl(0)); }
EOF

$CXX --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib
$CXX --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo
$t/exe | grep -q '^34 139 3$'
$CXX -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo
$t/exe2 | grep -q '^34 139 3$'
otool -L $t/exe2 > $t/libs
not grep -q libfoo $t/libs
