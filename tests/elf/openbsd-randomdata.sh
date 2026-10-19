#!/usr/bin/env bash
. $(dirname $0)/common.inc

# .openbsd.randomdata is in RELRO. Linux ignores PT_OPENBSD_RANDOMIZE,
# but it applies PT_GNU_RELRO, so the section becomes read-only after
# relocation. read(2) into a read-only buffer fails with EFAULT.
cat <<EOF | $CC -c -o $t/a.o -xc -
#include <errno.h>
#include <stdio.h>
#include <unistd.h>

long foo __attribute__((section(".openbsd.randomdata")));

int main() {
  int fd[2];
  pipe(fd);
  write(fd[1], "x", 1);
  if (read(fd[0], &foo, 1) == -1 && errno == EFAULT)
    printf("read-only\n");
  else
    printf("writable\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o
$QEMU $t/exe1 | grep read-only

# readelf older than binutils 2.36 doesn't know PT_OPENBSD_RANDOMIZE.
readelf -lW $t/exe1 | grep -E 'OPENBSD_RANDOM|LOOS\+0x5a3dbe6'

$CC -B. -o $t/exe2 $t/a.o -Wl,-z,norelro
$QEMU $t/exe2 | grep writable
