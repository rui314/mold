#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/exe -xc - -Wl,-adhoc_codesign
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

"$t"/exe | fgrep -q 'Hello world'

echo OK
