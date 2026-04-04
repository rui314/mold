#!/bin/bash
. $(dirname $0)/common.inc

[ "$(uname)" = FreeBSD ] && skip

# Test that --no-allow-shlib-undefined rejects hidden-visibility symbols
# as valid definitions for shared library undefined references. A hidden
# symbol in a static object cannot satisfy a DSO's undefined reference
# at runtime, so the linker must report an error.

cat <<EOF | $CC -c -o $t/a.o -xc - -fPIC
__attribute__((visibility("hidden"))) void foo() {}
EOF

cat <<EOF | $CC -B. -shared -fPIC -o $t/libbar.so -xc -
void foo();
void bar() { foo(); }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void bar();
int main() { bar(); }
EOF

not $CC -B. -o $t/exe $t/b.o $t/a.o -Wl,--no-allow-shlib-undefined \
  -L$t -lbar >& $t/log
grep -F 'undefined symbol: foo' $t/log
