#!/bin/bash
source "$(dirname "$0")"/common.inc
source "$(dirname "$0")"/dtrace.inc

# Each provider gets a DOF section of its own. A probe has an instance
# per function it has sites in, named after the name of the function's
# subsection less its leading underscore: the static functions of one
# name in two files share one. A section is named after the provider,
# cut to 15 bytes; one that would have an earlier one's name has its
# last byte replaced by '0', '1' and so on. (The test leaves out the
# order of the sections, which ld-prime has by a hash table of the
# providers, and of the probes and instances in each.)
cat > $t/p.d <<EOF
provider zeta {
  probe start(int);
  probe stop();
  probe only__enabled(int);
};
provider alpha {
  probe go(long, char *);
  probe zz();
  probe aa(int);
};
provider longprovidera { probe p(); };
provider longproviderb { probe p(); };
EOF
dtrace_header p

cat > $t/a.c <<EOF
#include "p.h"
static void helper(int x) { ZETA_START(x); ALPHA_ZZ(); }
void fa(int x) {
  if (ZETA_ONLY_ENABLED_ENABLED()) ZETA_STOP();
  ZETA_START(x);
  helper(x + 1);
  ALPHA_GO(x, "a");
  ZETA_START(x * 2);
  LONGPROVIDERA_P();
}
void fb(int x) {
  if (ALPHA_AA_ENABLED()) ALPHA_AA(x);
  ZETA_STOP();
  helper(x);
  ZETA_ONLY_ENABLED(7);
  LONGPROVIDERB_P();
}
EOF

cat > $t/b.c <<EOF
#include "p.h"
static void helper(int x) { ZETA_START(x); ALPHA_GO(x, "b"); }
void fa(int);
void fb(int);
void fc(int x) {
  ALPHA_AA(x);
  if (ZETA_START_ENABLED()) ZETA_START(3);
  helper(x);
}
int main(int argc, char **argv) {
  fc(argc);
  fa(argc);
  fb(argc);
  ALPHA_ZZ();
  return 0;
}
EOF
$CC -o $t/a.o -c $t/a.c
$CC -o $t/b.o -c $t/b.c
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe

# The sections' names, and each provider's DOF, by provider.
dof_dump $t/exe > $t/dof
grep '^dof' $t/dof | awk '{ print $2 }' | sort > $t/names
cat > $t/expected <<EOF
__dof_alpha
__dof_longprov0
__dof_longprovi
__dof_zeta
EOF
diff $t/names $t/expected

awk '/^dof/ { p = $3; sub(/^dof [^ ]* /, "dof ") } { print p, $0 }' $t/dof |
  sort | cut -d' ' -f2- > $t/by-provider
cat > $t/expected <<EOF
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
dof alpha flags 0xf align 0
probe aa(int) in fb: 1 sites, 1 tests
probe aa(int) in fc: 1 sites, 0 tests
probe go(long, char *) in fa: 1 sites, 0 tests
probe go(long, char *) in helper: 1 sites, 0 tests
probe zz() in helper: 1 sites, 0 tests
probe zz() in main: 1 sites, 0 tests
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
dof longprovidera flags 0xf align 0
probe p() in fa: 1 sites, 0 tests
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
dof longproviderb flags 0xf align 0
probe p() in fb: 1 sites, 0 tests
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
dof zeta flags 0xf align 0
probe only-enabled(int) in fa: 0 sites, 1 tests
probe only-enabled(int) in fb: 1 sites, 0 tests
probe start(int) in fa: 2 sites, 0 tests
probe start(int) in fc: 1 sites, 1 tests
probe start(int) in helper: 2 sites, 0 tests
probe stop() in fa: 1 sites, 0 tests
probe stop() in fb: 1 sites, 0 tests
EOF
diff $t/by-provider $t/expected
