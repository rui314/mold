#!/bin/bash
source "$(dirname "$0")"/common.inc
source "$(dirname "$0")"/dtrace.inc

# The DOF is made before dead stripping, of the probe sites of all
# functions, and is a root that refers to each: -dead_strip keeps every
# function with a site, and what it calls. -why_live names the DOF's
# subsection after the provider, the linker's "dtrace-file" its file.
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
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip \
  -Wl,-why_live,_dead1 -Wl,-why_live,_callee 2> $t/log
$t/exe
nm $t/exe > $t/syms
grep -q '_dead1$' $t/syms
grep -q '_dead2$' $t/syms
grep -q '_callee$' $t/syms
not grep -q '_unused$' $t/syms

dof_dump $t/exe > $t/dof
cat > $t/expected <<EOF
dof __dof_stab stab flags 0xf align 0
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
probe x(int) in live: 1 sites, 0 tests
probe x(int) in dead2: 1 sites, 0 tests
probe x(int) in dead1: 1 sites, 0 tests
EOF
diff $t/dof $t/expected

grep -v '^+' $t/log | sed 's| from .*/| from |' > $t/why
cat > $t/expected <<EOF
_dead1 from a.o
  l__dtrace_dof_for_provider_stab from dtrace-file
_callee from a.o
  _dead2 from a.o
    l__dtrace_dof_for_provider_stab from dtrace-file
EOF
diff $t/why $t/expected

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
$t/exe2
nm $t/exe2 > $t/syms2
[ "$(grep -E '_plain_[ab]$' $t/syms2 | cut -d' ' -f1 | uniq | wc -l)" -eq 1 ]
[ "$(grep -E '_twin_[ab]$' $t/syms2 | cut -d' ' -f1 | uniq | wc -l)" -eq 2 ]
