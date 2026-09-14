#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The linker has to synthesize the _savefpr_N, _restfpr_N, _savevr_N and
# _restvr_N routines that GCC calls from function prologues and epilogues
# with -Os, just like the _savegpr0_N ones.

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl foo
foo:
  bl _savefpr_14
  bl _restfpr_14
  bl _savefpr_29
  bl _restfpr_29
  bl _savevr_20
  bl _restvr_20
  bl _savevr_31
  bl _restvr_31
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o
$OBJDUMP -d $t/exe1 > $t/log1
grep -A1 '<_savefpr_14>:' $t/log1 | grep -F 'stfd    f14,-144(r1)'
grep -A2 '<_restfpr_29>:' $t/log1 | grep -F 'lfd     f29,-24(r1)'
grep -A2 '<_savevr_20>:' $t/log1 | grep -F 'stvx    v20,r12,r0'
grep -A2 '<_restvr_31>:' $t/log1 | grep -F 'lvx     v31,r12,r0'

# A function that keeps many floating-point values live across calls
# makes GCC use the routines with -Os.
cat <<EOF | $CC -Os -o $t/c.o -c -xc -
double g(double);
double out[20];

void f(double a) {
  double r[20];
  for (int i = 0; i < 20; i++)
    r[i] = g(a + i);
  for (int i = 0; i < 20; i++)
    out[i] = r[i];
}
EOF

cat <<EOF | $CC -Os -o $t/d.o -c -xc -
#include <stdio.h>
extern double out[20];
void f(double);
double g(double x) { return x * 2; }

int main() {
  f(1);
  double sum = 0;
  for (int i = 0; i < 20; i++)
    sum += out[i];
  printf("%g\n", sum);
}
EOF

$CC -B. -o $t/exe2 $t/c.o $t/d.o
$QEMU $t/exe2 | grep '^420$'
