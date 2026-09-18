#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A quoted name is matched literally, so the brackets in `operator[]`
# are not a character class. We don't use `operator delete[]` because
# FreeBSD's libcxxrt demangles it as `operator delete []`.

cat <<EOF | $CXX -fPIC -c -o $t/a.o -xc++ -
struct S { int operator[](int); };
int S::operator[](int i) { return i; }
extern "C" void foo() {}
extern "C" void bar() {}
EOF

cat <<'EOF' > $t/b.ver
VER1 {
  global:
    extern "C++" { "S::operator[](int)"; };
    "foo";
  local: *;
};
EOF

$CXX -B. -shared -Wl,--version-script=$t/b.ver -o $t/c.so $t/a.o

readelf --dyn-syms $t/c.so > $t/log
grep -F '_ZN1SixEi@@VER1' $t/log
grep -F 'foo@@VER1' $t/log
not grep -F 'bar' $t/log
