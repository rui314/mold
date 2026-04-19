#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
int foo();
int main() { foo(); }
EOF

not $CC -B. -o $t/exe $t/a.o |& grep 'undefined.*foo'

not $CC -B. -o $t/exe $t/a.o -Wl,-unresolved-symbols=report-all |&
  grep 'undefined.*foo'

$CC -B. -o $t/exe $t/a.o -Wl,-unresolved-symbols=ignore-all

readelf --dyn-syms $t/exe | not grep -w foo

$CC -B. -o $t/exe $t/a.o -Wl,-unresolved-symbols=report-all \
  -Wl,--warn-unresolved-symbols |& grep 'undefined.*foo'

$CC -B. -o $t/exe $t/a.o -Wl,-unresolved-symbols=ignore-in-object-files |&
  not grep 'undefined.*foo'

not $CC -B. -o $t/exe $t/a.o -Wl,-unresolved-symbols=ignore-in-shared-libs |&
  grep 'undefined.*foo'
