#!/bin/bash
source "$(dirname "$0")"/common.inc
source "$(dirname "$0")"/dtrace.inc

# The DOF lists the probe sites of the code dead stripping leaves:
# -dead_strip removes a function nothing calls with its probe sites, and
# what only it calls. (ld-prime makes the DOF before dead stripping, of
# the sites of all functions, and keeps every function with a site, and
# what it calls, for probes that can never fire.)
cat > $t/p.d <<EOF
provider stab {
  probe x(int);
};
EOF
dtrace_header p

cat > $t/a.c <<EOF
#include "p.h"
void callee(void) {}
void unused(void) {}
void dead1(void) { STAB_X(1); }
void dead2(void) { callee(); STAB_X(2); }
void live(void) { STAB_X(3); }
int main() { live(); return 0; }
EOF
$CC -o $t/a.o -c $t/a.c
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip
$RUN $t/exe
nm $t/exe > $t/syms
grep -q '_live$' $t/syms
not grep -q '_dead1$' $t/syms
not grep -q '_dead2$' $t/syms
not grep -q '_callee$' $t/syms
not grep -q '_unused$' $t/syms

dof_dump $t/exe > $t/dof
cat > $t/expected <<EOF
dof __dof_stab stab flags 0xf align 0
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
probe x(int) in live: 1 sites, 0 tests
EOF
diff $t/dof $t/expected

# Without -dead_strip, every function's sites are listed.
$CC --ld-path=$mold -o $t/exe3 $t/a.o
dof_dump $t/exe3 > $t/dof3
sort -o $t/dof3 $t/dof3
cat > $t/expected <<EOF
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
dof __dof_stab stab flags 0xf align 0
probe x(int) in dead1: 1 sites, 0 tests
probe x(int) in dead2: 1 sites, 0 tests
probe x(int) in live: 1 sites, 0 tests
EOF
diff $t/dof3 $t/expected

# Identical functions are folded, but not those with a probe site.
cat > $t/b.c <<EOF
#include "p.h"
static __attribute__((noinline)) void twin_a(int x) { STAB_X(x); }
static __attribute__((noinline)) void twin_b(int x) { STAB_X(x); }
static __attribute__((noinline)) int plain_a(int x) { return x * 3 + 1; }
static __attribute__((noinline)) int plain_b(int x) { return x * 3 + 1; }
int main(int argc, char **argv) {
  twin_a(argc);
  twin_b(argc);
  return plain_a(argc) - plain_b(argc);
}
EOF
$CC -o $t/b.o -c $t/b.c
$CC --ld-path=$mold -o $t/exe2 $t/b.o -Wl,-deduplicate
$RUN $t/exe2
nm $t/exe2 > $t/syms2
[ "$(grep -E '_plain_[ab]$' $t/syms2 | cut -d' ' -f1 | uniq | wc -l)" -eq 1 ]
[ "$(grep -E '_twin_[ab]$' $t/syms2 | cut -d' ' -f1 | uniq | wc -l)" -eq 2 ]
