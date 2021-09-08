#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

# Skip if libc is musl because musl does not support GNU FUNC
echo 'int main() {}' | cc -o $t/exe -xc -
ldd $t/exe | grep -q ld-musl && { echo OK; exit; }

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

__attribute__((ifunc("resolve_foobar")))
static void foobar(void);

static void real_foobar(void) {
  printf("Hello world\n");
}

typedef void Func();

static Func *resolve_foobar(void) {
  return real_foobar;
}

int main() {
  foobar();
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o
$t/exe | grep -q 'Hello world'

echo OK
