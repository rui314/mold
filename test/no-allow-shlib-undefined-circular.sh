#!/bin/bash
. $(dirname $0)/common.inc

[ "$(uname)" = FreeBSD ] && skip

cat <<EOF | $CC -B. -shared -fPIC -o $t/libfoo.so -xc -
void foo() {}
EOF

cat <<EOF | $CC -B. -shared -fPIC -o $t/libbar.so -xc - -L$t -lfoo
void foo();
void bar() { foo(); }
EOF

cat <<EOF | $CC -B. -shared -fPIC -o $t/libfoo.so -xc - -L$t -lbar
void baz();
void foo() { baz(); }
EOF

cat <<EOF | $CC -c -o $t/a.o -c -xc -
int bar();
int main() { bar(); }
EOF

not $CC -B. -o $t/exe1 $t/a.o -Wl,--no-allow-shlib-undefined -L$t -lfoo -lbar |&
  grep 'libfoo.so: --no-allow-shlib-undefined: undefined symbol: baz'
