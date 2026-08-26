#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A quoted name is matched literally, so the brackets in
# `operator delete[]` are not a character class.

cat <<EOF | $CXX -fPIC -c -o $t/a.o -xc++ -
void operator delete[](void *p) noexcept {}
void operator delete(void *p) noexcept {}
extern "C" void foo() {}
extern "C" void bar() {}
EOF

cat <<'EOF' > $t/b.ver
VER1 {
  global:
    extern "C++" {
      "operator delete[](void*)";
      "operator delete(void*)";
    };
    "foo";
  local: *;
};
EOF

$CXX -B. -shared -Wl,--version-script=$t/b.ver -o $t/c.so $t/a.o

readelf --dyn-syms $t/c.so > $t/log
grep -F '_ZdaPv@@VER1' $t/log
grep -F '_ZdlPv@@VER1' $t/log
grep -F 'foo@@VER1' $t/log
not grep -F 'bar' $t/log
