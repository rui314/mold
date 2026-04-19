#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -m32 || skip

mkdir -p $t/foo

echo 'char hello[] = "Hello world";' | $CC -shared -o $t/libbar.so -m32 -xc -
echo 'char hello[] = "Hello world";' | $CC -shared -o $t/foo/libbar.so -xc -

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
extern char hello[];
int main() {
  printf("%s\n", hello);
}
EOF

cat <<EOF > $t/b.script
INPUT(libbar.so)
EOF

cd $t

$CC -B$OLDPWD -o exe1 -Lfoo a.o b.script
LD_LIBRARY_PATH=. $QEMU ./exe1 | grep 'Hello world'

$CC -B$OLDPWD -o exe2 -Lfoo b.script a.o
LD_LIBRARY_PATH=. $QEMU ./exe2 | grep 'Hello world'
