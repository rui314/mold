#!/bin/bash
source "$(dirname "$0")"/common.inc

# Before macOS 10.8, dyld called no main: an x86-64 executable for an
# older macOS starts from LC_UNIXTHREAD at crt1.o's "start", which clang
# links for it (-lcrt1.10.6.o), as a static executable does. A stack it
# asks for is a segment of its own, and it may say where. An arm64
# executable starts from LC_MAIN whatever the target.
sdk=$(xcrun --show-sdk-path)
cat > $t/a.c <<EOF
#include <stdio.h>
int main() { printf("Hello world\n"); return 0; }
EOF

if [ $ARCH = x86_64 ]; then
  $CC -c $t/a.c -o $t/a107.o -mmacosx-version-min=10.7
  $CC --ld-path=$mold -o $t/exe1 $t/a107.o -mmacosx-version-min=10.7
  $RUN $t/exe1 | grep 'Hello world'
  otool -l $t/exe1 > $t/lc1
  not grep -q 'cmd LC_MAIN' $t/lc1
  start=$(nm $t/exe1 | awk '$3 == "start" { print $1 }' | sed 's/^0*//')
  grep -A12 'cmd LC_UNIXTHREAD' $t/lc1 | grep "rip 0x0*$start\$"

  $CC --ld-path=$mold -o $t/exe2 $t/a107.o -mmacosx-version-min=10.7 \
    -Wl,-stack_size,0x100000,-stack_addr,0x7f0000000000
  otool -l $t/exe2 > $t/lc2
  grep -A2 'segname __UNIXSTACK' $t/lc2 | grep 'vmaddr 0x00007efffff00000'
  grep -q 'rsp 0x00007f0000000000' $t/lc2

  $CC -c $t/a.c -o $t/a108.o -mmacosx-version-min=10.8
  $CC --ld-path=$mold -o $t/exe3 $t/a108.o -mmacosx-version-min=10.8
  $RUN $t/exe3 | grep 'Hello world'
  otool -l $t/exe3 > $t/lc3
  grep -q 'cmd LC_MAIN' $t/lc3
  not grep -q 'cmd LC_UNIXTHREAD' $t/lc3
else
  $CC -c $t/a.c -o $t/a.o
  $mold -arch $ARCH -syslibroot $sdk -o $t/exe4 $t/a.o -lSystem \
    -platform_version macos 10.7 27.0 2> /dev/null
  otool -l $t/exe4 > $t/lc4
  grep -q 'cmd LC_MAIN' $t/lc4
  not grep -q 'cmd LC_UNIXTHREAD' $t/lc4
fi
