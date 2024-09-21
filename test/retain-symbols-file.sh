#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
static void foo() {}
void bar() {}
void baz() {}
int main() { foo(); }
EOF

cat <<EOF > $t/symbols
foo
baz
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--retain-symbols-file=$t/symbols
readelf -W --symbols $t/exe > $t/log

! grep -q ' foo$' $t/log || false
! grep -q ' bar$' $t/log || false
! grep -q ' main$' $t/log || false

grep -q ' baz$' $t/log
