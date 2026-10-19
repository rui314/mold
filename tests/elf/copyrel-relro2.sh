#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIE -
#include <stdio.h>
#include <unistd.h>

extern char readonly[100];
extern char readwrite[100];

// read(2) into a read-only buffer fails with EFAULT instead of crashing.
static int is_writable(char *p) {
  int fd[2];
  pipe(fd);
  write(fd[1], "x", 1);
  return read(fd[0], p, 1) == 1;
}

int main() {
  printf("writable %d %d\n", is_writable(readwrite), is_writable(readonly));
}
EOF

cat <<EOF | $CC -B. -fPIC -shared -o $t/b.so -xc -
__attribute__((section (".data.rel.ro"))) char readonly[100] = "abc";
char readwrite[100] = "abc";
EOF

$CC -B. $t/a.o $t/b.so -o $t/exe -no-pie
$QEMU $t/exe | grep '^writable 1 0$'
