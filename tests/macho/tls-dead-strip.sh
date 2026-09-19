#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <assert.h>
#include <pthread.h>

_Thread_local int live_data = 5;
_Thread_local int live_bss;
_Thread_local int dead_data = 7;
_Thread_local int dead_bss;
__attribute__((used)) _Thread_local int retained_data = 9;

void *worker(void *arg) {
  assert(live_data == 5);
  assert(live_bss == 0);
  live_data = 11;
  live_bss = 13;
  return 0;
}

int main() {
  pthread_t thread;
  assert(pthread_create(&thread, 0, worker, 0) == 0);
  assert(pthread_join(thread, 0) == 0);
  assert(live_data == 5);
  assert(live_bss == 0);
}
EOF

# Without dead stripping, both unused TLS descriptors and their storage remain.
$CC --ld-path=$mold -o $t/all $t/a.o
nm $t/all > $t/all.syms
for sym in dead_data dead_bss; do
  grep -q " _${sym}\$" $t/all.syms
  grep -Fq " _${sym}\$tlv\$init" $t/all.syms
done

# Referenced TLS remains initialized and isolated between threads. Unused TLS
# descriptors must not keep themselves or their backing storage alive.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip
$t/exe
nm $t/exe > $t/syms
not grep -q ' _dead_' $t/syms
for sym in live_data live_bss retained_data; do
  grep -q " _${sym}\$" $t/syms
  grep -Fq " _${sym}\$tlv\$init" $t/syms
done

# Explicit roots and dylib exports still retain otherwise unused TLS.
$CC --ld-path=$mold -o $t/forced $t/a.o -Wl,-dead_strip,-u,_dead_data
nm $t/forced > $t/forced.syms
grep -q ' _dead_data$' $t/forced.syms
grep -Fq ' _dead_data$tlv$init' $t/forced.syms
not grep -q ' _dead_bss' $t/forced.syms

$CC --ld-path=$mold -shared -o $t/libtls.dylib $t/a.o -Wl,-dead_strip \
  -install_name $PWD/$t/libtls.dylib
nm $t/libtls.dylib > $t/dylib.syms
for sym in dead_data dead_bss; do
  grep -q " _${sym}\$" $t/dylib.syms
  grep -Fq " _${sym}\$tlv\$init" $t/dylib.syms
done
