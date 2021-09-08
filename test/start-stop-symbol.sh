#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<'EOF' | clang -c -o $t/a.o -xc -
__attribute__((section("foo")))
char data[] = "section foo";
EOF

ar rcs $t/b.a $t/a.o

cat <<EOF | clang -c -o $t/c.o -xc -
#include <stdio.h>

extern char data[];
extern char __start_foo[];
extern char __stop_foo[];

int main() {
  printf("%.*s %s\n", (int)(__stop_foo - __start_foo), __start_foo, data);
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.o $t/b.a
$t/exe | grep -q 'section foo section foo'

clang -fuse-ld=$mold -o $t/exe $t/c.o $t/b.a -Wl,-gc-sections
$t/exe | grep -q 'section foo section foo'

echo OK
