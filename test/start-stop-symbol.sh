#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<'EOF' | clang -c -o $t/a.o -x assembler -
.section foo,"a",@progbits
.ascii "section foo"
.text
.globl bar
bar:
  mov $3, %eax
  ret
EOF

ar rcs $t/b.a $t/a.o

cat <<EOF | clang -c -o $t/c.o -xc -
#include <stdio.h>

extern char __start_foo[];
extern char __stop_foo[];

int bar();

int main() {
  printf("%.*s %d\n", (int)(__stop_foo - __start_foo), __start_foo, bar());
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.o $t/b.a
$t/exe | grep -q 'section foo 3'

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.o $t/b.a -Wl,-gc-sections
$t/exe | grep -q 'section foo 3'

echo OK
