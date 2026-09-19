#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
#include <pthread.h>

_Thread_local int x = 5;
_Thread_local int y;
_Thread_local char big[100] = "hello";

void *th(void *arg) {
  x = 10;
  y = 20;
  return 0;
}

int main() {
  y = 7;
  pthread_t t;
  pthread_create(&t, 0, th, 0);
  pthread_join(t, 0);
  printf("%d %d %s\n", x, y, big);
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep '5 7 hello'
# Local thread-locals need no __thread_ptrs indirection at all.
objdump -h $t/exe > $t/sections
! grep -q __thread_ptrs $t/sections || false
