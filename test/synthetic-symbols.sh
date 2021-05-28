#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -x assembler -
.section foo,"a",@progbits
.ascii "section foo"
EOF

# Test synthetic symbols

cat <<EOF | clang -c -o $t/b.o -xc -
#include <stdio.h>
#include <string.h>

extern char __ehdr_start[];
extern char __executable_start[];
extern char __start_foo[];
extern char __stop_foo[];

int main() {
  printf("__ehdr_start=%p\n", &__ehdr_start);
  printf("__executable_start=%p\n", &__executable_start);
  printf("%.*s\n", (int)(__stop_foo - __start_foo), __start_foo);
}
EOF

clang -fuse-ld=`pwd`/../mold -Wl,--image-base=0x40000 -o $t/exe $t/a.o $t/b.o
$t/exe > $t/log

grep -q '^__ehdr_start=0x40000$' $t/log
grep -q '^__executable_start=0x40000$' $t/log
grep -q '^section foo$' $t/log

# Make sure that synthetic symbols overwrite existing ones

cat <<EOF | clang -c -o $t/c.o -xc -
#include <stdio.h>
#include <string.h>

char __ehdr_start[] = "foo";
char __executable_start[] = "foo";
char __start_foo[] = "foo";
char __stop_foo[] = "foo";

int main() {
  printf("__ehdr_start=%p\n", &__ehdr_start);
  printf("__executable_start=%p\n", &__executable_start);
  printf("%.*s\n", (int)(__stop_foo - __start_foo), __start_foo);
}
EOF

clang -fuse-ld=`pwd`/../mold -Wl,--image-base=0x40000 -o $t/exe $t/a.o $t/c.o
$t/exe > $t/log

grep -q '^__ehdr_start=0x40000$' $t/log
grep -q '^__executable_start=0x40000$' $t/log
grep -q '^section foo$' $t/log

echo OK
