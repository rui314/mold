#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int answer() { return 42; }
EOF2

$CC --ld-path=$mold -bundle -o $t/foo.bundle $t/a.o

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <dlfcn.h>
#include <stdio.h>
int main(int argc, char **argv) {
  void *h = dlopen(argv[1], RTLD_NOW);
  if (!h) { printf("dlopen failed: %s\n", dlerror()); return 1; }
  int (*fn)(void) = dlsym(h, "answer");
  if (!fn) { printf("dlsym failed\n"); return 1; }
  printf("%d\n", fn());
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/b.o
$t/exe $t/foo.bundle | grep '^42$'
