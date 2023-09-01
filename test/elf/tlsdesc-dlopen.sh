#!/bin/bash
. $(dirname $0)/common.inc

if [ $MACHINE = x86_64 -o $MACHINE = arm ]; then
  dialect=gnu2
elif [ $MACHINE = aarch64 ]; then
  dialect=desc
else
  skip
fi

cat <<EOF | $GCC -fPIC -mtls-dialect=$dialect -c -o $t/a.o -xc -
static _Thread_local int foo[10000] = { 3, [9999] = 5 };

int get_foo(int idx) { return foo[idx]; }
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $GCC -fPIC -mtls-dialect=$dialect -c -o $t/c.o -xc -
#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
  void *handle = dlopen(argv[1], RTLD_LAZY);
  if (!handle) {
    fprintf(stderr, "dlopen failed: %s: %s: \n", argv[1], dlerror());
    exit(1);
  }

  int (*get)(int) = dlsym(handle, "get_foo");
  assert(get);

  printf("%d %d %d\n", get(0), get(1), get(9999));
}
EOF

$CC -B. -o $t/exe $t/c.o -ldl
$QEMU $t/exe $t/b.so | grep -q '3 0 5'
