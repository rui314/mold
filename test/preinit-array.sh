#!/bin/bash
. $(dirname $0)/common.inc

# We should not create a PREINIT_ARRAY .dynamic entry by default
cat <<EOF | $CC -o $t/a.o -c -xc -
void _start() {}
EOF

./mold -o $t/exe1 $t/a.o
readelf -W --dynamic $t/exe1 | not grep PREINIT_ARRAY

cat <<EOF | $CC -o $t/b.o -c -xc -
void preinit_fn() {}
int main() {}

__attribute__((section(".preinit_array")))
void *preinit[] = { preinit_fn };
EOF

# We create a PREINIT_ARRAY .dynamic entry if necessary
$CC -B. -o $t/exe2 $t/b.o
readelf -W --dynamic $t/exe2 | grep PREINIT_ARRAY
