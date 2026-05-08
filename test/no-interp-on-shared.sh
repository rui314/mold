#!/bin/bash
. $(dirname $0)/common.inc

# Object with an entry point: used for PIE / runnable-.so cases
cat <<EOF | $CC -fPIC -xc -c -o $t/a.o -
void _start(void) {}
EOF

# Object without an entry point: used for the regular shared case
cat <<EOF | $CC -fPIC -xc -c -o $t/b.o -
int foo(void) { return 42; }
EOF

DL=/lib/ld.so

# Control: bare -shared (no --dynamic-linker) must NOT emit PT_INTERP or .interp
./mold -shared -o $t/control.so $t/b.o
readelf -l $t/control.so | not grep INTERP
readelf -S $t/control.so | not grep '\.interp'

# Bug case: -shared with --dynamic-linker must NOT emit PT_INTERP or .interp
./mold -shared --dynamic-linker=$DL -o $t/regular.so $t/b.o
readelf -l $t/regular.so | not grep INTERP
readelf -S $t/regular.so | not grep '\.interp'

# Regression: -pie executable still gets PT_INTERP and .interp
./mold -pie --dynamic-linker=$DL -e _start -o $t/pie-exe $t/a.o
readelf -l $t/pie-exe | grep INTERP
readelf -S $t/pie-exe | grep '\.interp'

# Regression: -shared -pie (runnable .so) still gets PT_INTERP and .interp -- BFD parity
./mold -shared -pie --dynamic-linker=$DL -e _start -o $t/runnable.so $t/a.o
readelf -l $t/runnable.so | grep INTERP
readelf -S $t/runnable.so | grep '\.interp'
