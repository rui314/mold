#!/bin/bash
source "$(dirname "$0")"/common.inc

# A GOT reference to a symbol defined in the same image relaxes to a
# direct address computation; one to a dylib symbol keeps the GOT.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int local_val = 42;
extern int dylib_val;
int *get_local() { return &local_val; }
int *get_dylib() { return &dylib_val; }
int main() { printf("%d %d\n", *get_local(), *get_dylib()); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int dylib_val = 7;
EOF
$CC --ld-path=$mold -shared -o $t/libb.dylib $t/b.o

$CC --ld-path=$mold -o $t/exe $t/a.o $t/libb.dylib
$t/exe | grep -q '^42 7$'

objdump -d $t/exe > $t/dis
if [ $ARCH = arm64 ]; then
  # local: adrp+add; dylib: adrp+ldr through the GOT
  sed -n '/<_get_local>:/,/ret/p' $t/dis > $t/local
  grep -q adrp $t/local && grep -q 'add' $t/local && ! grep -q 'ldr' $t/local
  sed -n '/<_get_dylib>:/,/ret/p' $t/dis > $t/dylib
  grep -q adrp $t/dylib && grep -q 'ldr' $t/dylib
else
  grep -q 'leaq.*_local_val' $t/dis
  grep -q 'movq.*rip' $t/dis
fi
