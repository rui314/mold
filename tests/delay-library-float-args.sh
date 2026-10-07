#!/bin/bash
source "$(dirname "$0")"/common.inc

# The call that first reaches a delayed dylib passes its arguments
# through the dlopen() of the dylib, which the dlopen helper must
# preserve, the floating-point ones too. ld-prime's x86-64 helper saves
# only the general-purpose registers, so with it addf() below first
# returns 0.5 (this test fails with ld-prime on x86-64).
cat <<EOF | $CC -o $t/fl.o -c -xc -
#include <math.h>
#include <stdio.h>
__attribute__((constructor)) static void init(void) {
  volatile double x = 1.5;
  char buf[64];
  snprintf(buf, sizeof buf, "%f %f", sin(x), cos(x));
}
double addf(double a, double b, double c, double d) { return a + b + c + d; }
EOF
$CC -o $t/libfl.dylib -shared $t/fl.o -Wl,-install_name,@rpath/libfl.dylib

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
double addf(double, double, double, double);
int main() {
  printf("%g\n", addf(1.0, 2.0, 3.0, 4.0));
  printf("%g\n", addf(1.0, 2.0, 3.0, 4.0));
}
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-delay_library,$t/libfl.dylib -Wl,-rpath,$t
$RUN $t/exe > $t/out
printf '10\n10\n' | cmp - $t/out
