#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -B. -shared -o $t/libB.so -c -xc -
void not_needed() { }
EOF
$AR

cat <<EOF > $t/libscript.a
GROUP(AS_NEEDED("$t/libB.so"))
EOF

$CC -B. -o $t/exe -L$t -lscript $t/a.o
$QEMU $t/exe | grep 'Hello world'

$CC -B. -o $t/exe -L$t -l:libscript.a $t/a.o
$QEMU $t/exe | grep 'Hello world'
