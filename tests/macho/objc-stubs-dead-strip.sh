#!/bin/bash
source "$(dirname "$0")"/common.inc

# -dead_strip leaves no _objc_msgSend$<selector> stub, selector reference
# or selector name of only dead code's.
if [ $ARCH = arm64 ]; then
  call() { echo "bl _objc_msgSend\$$1"; }
else
  call() { echo "callq _objc_msgSend\$$1"; }
fi
{
  echo '.subsections_via_symbols'
  echo '.text'
  echo '.globl _main'
  echo '.p2align 2'
  echo '_main:'
  call live
  echo 'ret'
  echo '.p2align 2'
  echo '_dead:'
  call dead
  echo 'ret'
} > $t/a.s
$CC -o $t/a.o -c $t/a.s

$CC --ld-path=$mold -o $t/exe $t/a.o -lobjc -Wl,-dead_strip
nm $t/exe > $t/syms
grep -q '_objc_msgSend\$live$' $t/syms
not grep -q '_objc_msgSend\$dead' $t/syms
otool -X -v -s __TEXT __objc_methname $t/exe > $t/names
grep -q live $t/names
not grep -q 'dead' $t/names

$CC --ld-path=$mold -o $t/exe2 $t/a.o -lobjc
nm $t/exe2 | grep -q '_objc_msgSend\$dead$'
