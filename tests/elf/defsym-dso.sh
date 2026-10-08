#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
void foo() {}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
void bar();
int main() { bar(); }
EOF

not $CC -B. -o $t/exe $t/c.o $t/b.so -Wl,-defsym=bar=foo |&
  grep 'undefined symbol: foo'
