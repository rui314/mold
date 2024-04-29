#!/bin/bash
. $(dirname $0)/common.inc

mkdir -p $t/foo

cat <<EOF | $CC -o $t/foo/a.o -c -xc -
int main() {}
EOF

cat <<EOF > $t/foo/b.script

INPUT(a.o)
EOF

$CC -B. -o $t/exe $t/foo/b.script
