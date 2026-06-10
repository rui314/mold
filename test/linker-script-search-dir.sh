#!/bin/bash
. $(dirname $0)/common.inc

mkdir -p $t/dir

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 3; }
EOF

rm -f $t/dir/libfoo.a
ar rcs $t/dir/libfoo.a $t/a.o

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int foo();
int main() { printf("%d\n", foo()); }
EOF

# SEARCH_DIR(dir) is equivalent to -L
cat <<EOF > $t/script
SEARCH_DIR($t/dir)
GROUP(-lfoo)
EOF

$CC -B. -o $t/exe $t/b.o -Wl,-T,$t/script
$QEMU $t/exe | grep 3
