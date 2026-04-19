#!/bin/bash
. $(dirname $0)/common.inc

nm mold | grep '__tsan_init' && skip
test_cflags -m32 || skip

mkdir -p $t/foo

cat <<EOF | $CC -o $t/a.o -c -xc - -m32
char hello[] = "Hello world";
EOF

cat <<EOF | $CC -o $t/foo/a.o -c -xc -
char hello[] = "Hello world";
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
extern char hello[];
int main() {
  printf("%s\n", hello);
}
EOF

cat <<EOF > $t/d.script
INPUT(a.o)
EOF

cd $t

$OLDPWD/ld -o e.o -r -Lfoo d.script c.o
$OLDPWD/ld -o f.o -r -Lfoo c.o d.script
