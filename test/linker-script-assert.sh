#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

echo 'ASSERT(SIZEOF(.text) > 0, "text must not be empty")' > $t/good
$CC -B. -o $t/exe $t/a.o -Wl,-T,$t/good

echo 'ASSERT(SIZEOF(.text) == 0, "this should fail")' > $t/bad
not $CC -B. -o $t/exe2 $t/a.o -Wl,-T,$t/bad |& grep 'this should fail'
