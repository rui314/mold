#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -fPIC -c -xc -
__attribute__((visibility("hidden"))) void foo();
int main() { foo(); }
EOF

not $CC -B. -o $t/b.so -shared $t/a.o |& grep 'undefined symbol: foo'

not $CC -B. -o $t/exe $t/a.o -Wl,-unresolved-symbols=ignore-all |&
  grep 'undefined symbol: foo'
