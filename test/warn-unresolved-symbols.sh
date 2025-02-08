#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
int foo();
int main() {
  foo();
}
EOF

not $CC -B. -o $t/exe $t/a.o 2>&1 | grep -q 'undefined symbol:.*foo'

$CC -B. -o $t/exe $t/a.o -Wl,-warn-unresolved-symbols 2>&1 \
  | grep -q 'undefined symbol:.*foo'

not $CC -B. -o $t/exe $t/a.o -Wl,-warn-unresolved-symbols \
  -Wl,--error-unresolved-symbols 2>&1 | grep -q 'undefined symbol:.*foo'
