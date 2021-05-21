#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
#include <stdio.h>

void foo() {
  printf("foo\n");
}
EOF

cat <<EOF | clang -c -o $t/b.o -xc -
#include <stdio.h>

void foo();

void __wrap_foo() {
  printf("wrap_foo\n");
}

int main() {
  foo();
}
EOF

cat <<EOF | clang -c -o $t/c.o -xc -
#include <stdio.h>

void __real_foo();

int main() {
  __real_foo();
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '^foo$'

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.o -Wl,-wrap,foo
$t/exe | grep -q '^wrap_foo$'

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/c.o -Wl,-wrap,foo
$t/exe | grep -q '^foo$'

echo OK
