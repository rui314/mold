#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
void hello() { printf("Hello world\n"); }
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
void hello();
int main() { hello(); }
EOF

cat <<EOF > $t/filelist
$t/a.o
$t/b.o
EOF

cc --ld-path=./ld64 -o $t/exe -Wl,-filelist,$t/filelist
$t/exe | grep -q 'Hello world'

echo OK
