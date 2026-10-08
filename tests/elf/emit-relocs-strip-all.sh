#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
int main() { return 0; }
EOF

not ./mold -o $t/exe $t/a.o --emit-relocs --strip-all |&
  grep -F -- '--strip-all may not be used with --emit-relocs'

./mold -r -o $t/b.o $t/a.o --strip-all
