#!/bin/bash
source "$(dirname "$0")"/common.inc
source "$(dirname "$0")"/dtrace.inc

# A dtrace -h header has a probe site call an undefined
# ___dtrace_probe$... function and an is-enabled test call
# ___dtrace_isenabled$...; the link turns each into a nop (the test into
# a zeroing of its result) and describes them in a DOF section,
# __TEXT,__dof_<provider>, after the other sections of __TEXT. None of
# the ___dtrace_ symbols is left in the image. (The probes' order is
# left out: ld-prime has them by name, mold by their first sites.)
cat > $t/p.d <<EOF
provider myapp {
  probe request__start(int, char *);
  probe request__done(int);
  probe noargs();
};
EOF
dtrace_header p

cat > $t/a.c <<EOF
#include "p.h"
#include <stdio.h>
int main(int argc, char **argv) {
  MYAPP_REQUEST_START(argc, argv[0]);
  if (MYAPP_REQUEST_DONE_ENABLED())
    MYAPP_REQUEST_DONE(argc + 1);
  MYAPP_NOARGS();
  printf("Hello world\n");
  return 0;
}
EOF
$CC -o $t/a.o -c $t/a.c
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-map,$t/map
$t/exe | grep -q 'Hello world'

dof_dump $t/exe > $t/dof
sort -o $t/dof $t/dof
cat > $t/expected <<EOF
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
dof __dof_myapp myapp flags 0xf align 0
probe noargs() in main: 1 sites, 0 tests
probe request-done(int) in main: 1 sites, 1 tests
probe request-start(int, char *) in main: 1 sites, 0 tests
EOF
diff $t/dof $t/expected

nm -m $t/exe > $t/syms
not grep -q dtrace $t/syms

# Last of __TEXT, but for __unwind_info, packed.
otool -l $t/exe | grep -A1 '^  sectname' | grep -v -- '--' | paste - - |
  awk '$4 == "__TEXT" { print $2 }' > $t/sects
tail -2 $t/sects | head -1 | grep -q __dof_myapp
tail -1 $t/sects | grep -q __unwind_info

# -map lists the DOF as the linker's.
dof=$(grep $'\t__TEXT\t__dof_myapp$' $t/map | cut -f1)
grep -q "^$dof"$'\t0x[0-9A-F]*\t\\[  0\\] ' $t/map
