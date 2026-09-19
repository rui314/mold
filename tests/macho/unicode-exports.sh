#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -xc - -o $t/functions.o
int café() { return 42; }
int cafê() { return 43; }
EOF
cat <<EOF | $CC -c -xc - -o $t/main.o
#include <stdio.h>
#include <dlfcn.h>
int café(), cafê();
int main() {
  int (*a)() = dlsym(RTLD_DEFAULT, "café");
  int (*b)() = dlsym(RTLD_DEFAULT, "cafê");
  printf("%d %d %d %d\n", café(), cafê(), a ? a() : -1, b ? b() : -1);
}
EOF
$CC --ld-path=$mold $t/main.o $t/functions.o -o $t/exe
$t/exe | grep '^42 43 42 43$'

# The input reader must also assemble byte fragments before decoding.
$CC --ld-path=$mold -dynamiclib $t/functions.o -o $t/libunicode.dylib
strip $t/libunicode.dylib
codesign -f -s - $t/libunicode.dylib
$CC --ld-path=$mold $t/main.o $t/libunicode.dylib -o $t/client
$t/client | grep '^42 43 42 43$'
