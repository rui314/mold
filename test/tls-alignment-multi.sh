#!/bin/bash
. $(dirname $0)/common.inc

# Test a tricky case of TLS alignment requirement where not only the virtual
# address of a symbol but also its offset against the TLS base address has to
# be aligned.
#
# On glibc, this issue requires a TLS model equivalent to global-dynamic in
# order to be triggered.

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
#include <assert.h>
#include <stdlib.h>

// .tdata
_Thread_local int x = 42;
// .tbss
__attribute__ ((aligned(64)))
_Thread_local int y = 0;

void *verify(void *unused) {
  assert((unsigned long)(&y) % 64 == 0);
  return NULL;
}
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
#include <pthread.h>
#include <dlfcn.h>
#include <assert.h>
void *(*verify)(void *);

int main() {
  void *handle = dlopen("a.so", RTLD_NOW);
  assert(handle);
  *(void**)(&verify) = dlsym(handle, "verify");
  assert(verify);

  pthread_t thread;

  verify(NULL);

  pthread_create(&thread, NULL, verify, NULL);
  pthread_join(thread, NULL);
}
EOF

$CC -B. -shared -o $t/a.so $t/a.o
$CC -B. -ldl -pthread -o $t/exe $t/b.o -Wl,-rpath,$t
$QEMU $t/exe