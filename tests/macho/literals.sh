#!/bin/bash
source "$(dirname "$0")"/common.inc

# Only x86-64 clang materializes doubles from __literal8; arm64 code
# synthesizes them with mov/movk sequences.
CC="cc -arch x86_64"
arch -x86_64 /usr/bin/true 2> /dev/null || skip

cat <<EOF | $CC -o $t/a.o -c -xc -
double pi1() { return 3.1415926535; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
double pi2() { return 3.1415926535; }
int main() {}
EOF

# The two identical 8-byte literals coalesce into one, and the merged
# pool lands in __TEXT,__const as ld64 places it (a final image has
# no __literal4/8/16 sections).
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
objdump -h $t/exe > $t/sections
grep -Eq ' __const\s+00000008\s' $t/sections
not grep -q '__literal8' $t/sections
