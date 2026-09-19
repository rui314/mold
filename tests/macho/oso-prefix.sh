#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -g
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o -g
nm -pa $t/exe1 | grep -q 'OSO /'

$CC --ld-path=$mold -o $t/exe2 $t/a.o -g -Wl,-oso_prefix,.
nm -pa $t/exe2 | grep -Eq 'OSO out'

$CC --ld-path=$mold -o $t/exe3 $t/a.o -g -Wl,-oso_prefix,"`pwd`/"
nm -pa $t/exe3 | grep -Eq 'OSO out'
