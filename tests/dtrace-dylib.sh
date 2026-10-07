#!/bin/bash
source "$(dirname "$0")"/common.inc
source "$(dirname "$0")"/dtrace.inc

# A dylib or a bundle describes its probe sites in a DOF section as an
# executable does, which dyld hands to the kernel as it loads the image.
# The DOF holds each site's distance from it, so nothing in it slides:
# it needs no fixup.
cat > $t/p.d <<EOF
provider lib {
  probe call(int);
};
EOF
dtrace_header p

cat > $t/a.c <<EOF
#include "p.h"
int lib_fn(int x) {
  LIB_CALL(x);
  return LIB_CALL_ENABLED() ? -1 : x + 1;
}
EOF
cat > $t/b.c <<EOF
int lib_fn(int);
int main() { return lib_fn(41) != 42; }
EOF
$CC -o $t/a.o -c $t/a.c
$CC -o $t/b.o -c $t/b.c

$CC --ld-path=$mold -o $t/libfoo.dylib -shared $t/a.o
$CC --ld-path=$mold -o $t/exe $t/b.o $t/libfoo.dylib
$RUN $t/exe
dof_dump $t/libfoo.dylib > $t/dof
cat > $t/expected <<EOF
dof __dof_lib lib flags 0xf align 0
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
probe call(int) in lib_fn: 1 sites, 1 tests
EOF
diff $t/dof $t/expected
dyld_info -fixups $t/libfoo.dylib > $t/fixups
not grep -q __dof $t/fixups
nm -m $t/libfoo.dylib > $t/syms
not grep -q dtrace $t/syms

$CC --ld-path=$mold -o $t/foo.bundle -bundle $t/a.o
dof_dump $t/foo.bundle > $t/dof2
diff $t/dof2 $t/expected
