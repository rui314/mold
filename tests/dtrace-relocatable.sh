#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output keeps every undefined symbol of its inputs, DTrace's
# too: also a provider's stability and typedefs symbols, which nothing
# relocates (the header names them by N_NO_DEAD_STRIP .reference) but
# the final link reads the provider's attributes from.
source "$(dirname "$0")"/dtrace.inc
cat > $t/p.d <<EOF
provider myapp {
  probe start(int, char *);
};
EOF
dtrace_header p

cat > $t/a.c <<EOF
#include "p.h"
int main(int argc, char **argv) {
  MYAPP_START(argc, argv[0]);
  return 0;
}
EOF
$CC -o $t/a.o -c $t/a.c
$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/log
grep -q '(undefined) external ___dtrace_probe\$myapp\$start\$v1\$696e74\$63686172202a$' $t/log
grep -q '(undefined) external ___dtrace_stability\$myapp\$v1\$1_1_0_1_1_0_1_1_0_1_1_0_1_1_0$' $t/log
grep -q '(undefined) external ___dtrace_typedefs\$myapp\$v2$' $t/log

# A final link of the output makes the DOF a link of the input does.
$CC --ld-path=$mold -o $t/exe $t/r.o
$t/exe
$CC --ld-path=$mold -o $t/exe2 $t/a.o
dof_dump $t/exe > $t/dof
dof_dump $t/exe2 > $t/dof2
diff $t/dof $t/dof2
grep -q '^probe start(int, char \*) in main: 1 sites, 0 tests$' $t/dof
$CC -o $t/exe3 $t/r.o
$t/exe3

# A weak reference stays one.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _f
_f:
  .reference ___dtrace_foo
  .weak_reference ___dtrace_bar
  .reference _generic_ref
  ret
EOF
$mold -r -arch $ARCH -o $t/r2.o $t/b.o
nm -m $t/r2.o > $t/log2
grep -q '(undefined) external ___dtrace_foo$' $t/log2
grep -q '(undefined) weak external ___dtrace_bar$' $t/log2
grep -q '(undefined) external _generic_ref$' $t/log2
