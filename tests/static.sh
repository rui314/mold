#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -static image (a kernel, a boot loader) runs without dyld, so it
# has no LC_MAIN for dyld to read: its thread starts from LC_UNIXTHREAD,
# a register state that is all zero but the program counter, which
# holds the entry point. macOS still runs such an x86-64 program; an
# arm64 one it refuses, so there only the load commands are checked.
if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl __start
.p2align 2
__start:
  mov x0, #42
  mov x16, #1
  svc #0x80
EOF
else
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl __start
__start:
  movl \$42, %edi
  movl \$0x2000001, %eax
  syscall
EOF
fi

$mold -arch $ARCH -static -e __start $t/a.o -o $t/exe
otool -l $t/exe > $t/lc
grep -q 'cmd LC_UNIXTHREAD' $t/lc
not grep -q 'cmd LC_MAIN' $t/lc
not grep -q 'cmd LC_LOAD_DYLINKER' $t/lc
entry=$(nm $t/exe | awk '$3 == "__start" { print $1 }')
if [ $ARCH = arm64 ]; then
  grep -q " pc 0x$entry" $t/lc
else
  grep -q "rip 0x$entry" $t/lc
  rc=0
  $t/exe || rc=$?
  [ $rc = 42 ]
fi
