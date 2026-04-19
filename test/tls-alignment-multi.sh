#!/bin/bash
. $(dirname $0)/common.inc

# Test a tricky case of TLS alignment requirement where not only the virtual
# address of a symbol but also its offset against the TLS base address has to
# be aligned.
#
# On glibc, this issue requires a TLS model equivalent to global-dynamic in
# order to be triggered.

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

// .tdata
_Thread_local int x = 42;

// .tbss
__attribute__((aligned(64))) _Thread_local int y;

void *test(void *unused) {
  printf("%p %lu", &y, (unsigned long)&y % 64);
  return NULL;
}
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
#include <stdio.h>
#include <pthread.h>
#include <dlfcn.h>

int main() {
  void *handle = dlopen("c.so", RTLD_NOW);
  void *(*test)(void *) = dlsym(handle, "test");
  pthread_t th;

  test(NULL);
  printf(" ");

  pthread_create(&th, NULL, test, NULL);
  pthread_join(th, NULL);
  printf("\n");
}
EOF

$CC -B. -shared -o $t/c.so $t/a.o
$CC -B. -ldl -pthread -o $t/exe $t/b.o -Wl,-rpath,$t
$QEMU $t/exe | grep -E '^0x[0-9a-f]+ 0 0x[0-9a-f]+ 0$'
