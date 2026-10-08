#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The linker has to synthesize the _savegpr0_N, _restgpr0_N, _savefpr_N
# and _restfpr_N routines that GCC calls from function prologues and
# epilogues with -Os.

cat <<EOF | $CC -Os -o $t/a.o -c -xc -
long g(long);
double h(double);
long out1[4];
double out2[4];

void f1(long a) {
  long b = g(a), c = g(b), d = g(c), e = g(d);
  out1[0] = a + b; out1[1] = b + c; out1[2] = c + d; out1[3] = d + e;
}

void f2(double a) {
  double b = h(a), c = h(b), d = h(c), e = h(d);
  out2[0] = a + b; out2[1] = b + c; out2[2] = c + d; out2[3] = d + e;
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

extern long out1[4];
extern double out2[4];
void f1(long);
void f2(double);

long g(long x) { return x * 2; }
double h(double x) { return x * 2; }

int main() {
  f1(1);
  f2(1);
  printf("%ld %ld %ld %ld %g %g %g %g\n", out1[0], out1[1], out1[2], out1[3],
         out2[0], out2[1], out2[2], out2[3]);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '^3 6 12 24 3 6 12 24$'
