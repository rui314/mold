#!/bin/bash
source "$(dirname "$0")"/common.inc

# The DOF of a provider: its name, the attributes its stability symbol
# gives, and each probe with its argument types and an instance for the
# function it has sites in, with the number of its sites and is-enabled
# tests, each slot pointing at its site (which dof_dump checks). The
# probes are sorted: ld-prime has them by name, mold in the order of
# their first sites.
if [ $ARCH = arm64 ]; then call=bl; else call=call; fi

cat > $t/a.s <<EOF
.globl _main
_main:
  $call ___dtrace_probe\$stab\$x\$v1\$696e74
  ret
.reference ___dtrace_stability\$stab\$v1\$5_5_4_1_1_0_1_1_0_5_6_5_7_3_2
.reference ___dtrace_typedefs\$stab\$v2
.subsections_via_symbols
EOF
$CC -o $t/a.o -c $t/a.s
$CC --ld-path=$mold -o $t/exe $t/a.o
dof_dump $t/exe > $t/dof
cat > $t/expected <<EOF
dof __dof_stab stab flags 0xf align 0
attrs 0x05050400 0x01010000 0x01010000 0x05060500 0x07030200
probe x(int) in main: 1 sites, 0 tests
EOF
diff $t/dof $t/expected

cat > $t/b.s <<EOF
.globl _main
_main:
  $call ___dtrace_probe\$myapp\$request__start\$v1\$696e74\$63686172202a
  $call ___dtrace_isenabled\$myapp\$request__done\$v1
  $call ___dtrace_probe\$myapp\$request__done\$v1\$696e74
  $call ___dtrace_probe\$myapp\$noargs\$v1
  ret
.reference ___dtrace_stability\$myapp\$v1\$1_1_0_1_1_0_1_1_0_1_1_0_1_1_0
.reference ___dtrace_typedefs\$myapp\$v2
.subsections_via_symbols
EOF
$CC -o $t/b.o -c $t/b.s
$CC --ld-path=$mold -o $t/exe2 $t/b.o
dof_dump $t/exe2 > $t/dof2
sort -o $t/dof2 $t/dof2
cat > $t/expected <<EOF
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
dof __dof_myapp myapp flags 0xf align 0
probe noargs() in main: 1 sites, 0 tests
probe request-done(int) in main: 1 sites, 1 tests
probe request-start(int, char *) in main: 1 sites, 0 tests
EOF
diff $t/dof2 $t/expected
