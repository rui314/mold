#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() { printf("Hello world\n"); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void hello();
int main() { hello(); }
EOF

cat <<EOF > $t/filelist
a.o
b.o
EOF

$CC --ld-path=$mold -o $t/exe -Xlinker -filelist -Xlinker $t/filelist,$t
$t/exe | grep -q 'Hello world'
