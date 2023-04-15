#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fno-PIE -o $t/a.o -c -xc -
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

int main() {
  void *handle = dlopen(NULL, RTLD_NOW);
  void *p = dlsym(handle, "memcpy");
  printf("%p %p\n", memcpy, p);
}
EOF

$CC -B. -o $t/exe $t/a.o -no-pie -ldl
$QEMU $t/exe | grep -Eq '^(.+) \1$'
