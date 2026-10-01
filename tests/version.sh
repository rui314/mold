#!/bin/bash
source "$(dirname "$0")"/common.inc

# The -v banner goes to stderr, as ld-prime's does, and stdout stays
# empty.
banner='mold-macho\|PROGRAM:ld'
$mold -v > $t/out 2> $t/err
[ ! -s $t/out ]
grep "$banner" $t/err

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -Wl,-v -o $t/exe $t/a.o > $t/out2 2> $t/err2
[ ! -s $t/out2 ]
grep "$banner" $t/err2
$t/exe | grep 'Hello world'
