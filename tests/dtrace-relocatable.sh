#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output keeps every undefined symbol of its inputs whose name
# starts with ___dtrace_, as ld-prime does, with n_desc 0: also a
# provider's stability and typedefs symbols, which nothing relocates
# (the header names them by N_NO_DEAD_STRIP .reference) but the final
# link reads the provider's attributes from.
command -v dtrace > /dev/null || skip
cat > $t/p.d <<EOF
provider myapp {
  probe start(int, char *);
};
EOF
dtrace -h -s $t/p.d -o $t/p.h >& /dev/null || skip

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

# Any name with the prefix, a weak reference too (which loses its
# flag); a .reference of another name goes.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _f
_f:
  .reference ___dtrace_foo
  .reference ___dtrace_
  .weak_reference ___dtrace_bar
  .reference ___dtrace
  .reference _generic_ref
  ret
EOF
$mold -r -arch $ARCH -o $t/r2.o $t/b.o
nm -m $t/r2.o > $t/log2
grep -q '(undefined) external ___dtrace_foo$' $t/log2
grep -q '(undefined) external ___dtrace_$' $t/log2
grep -q '(undefined) external ___dtrace_bar$' $t/log2
not grep -q '___dtrace$\|_generic_ref\|undefined.*no dead strip' $t/log2
