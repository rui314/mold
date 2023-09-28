#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = aarch64 ] || skip

cat <<EOF | $CC -c -o $t/a.o -fPIC -xassembler -
.global foo
.type foo, %function
.variant_pcs foo
foo:
  ret
EOF

$CC -B. -shared -o $t/b.so $t/a.o
readelf -W --dyn-syms $t/b.so | grep -Eq '\[VARIANT_PCS\].* foo$'

cat <<EOF | $CC -c -o $t/c.o -xc -
void foo();
int main() { foo(); }
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so
readelf -W --dynamic $t/exe | grep -q AARCH64_VARIANT_PCS
