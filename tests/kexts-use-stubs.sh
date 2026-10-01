#!/bin/bash
source "$(dirname "$0")"/common.inc

# An x86-64 kext calls its imports directly, kmutil filling in each call
# by an external BRANCH relocation. -kexts_use_stubs has the calls go
# through a __TEXT,__stubs entry instead, a jmp through the import's
# __got slot, which an external UNSIGNED relocation fills. An arm64
# kext calls through stubs either way, and the option changes nothing.
cat <<EOF | $CC -o $t/a.o -c -xc -O1 -mkernel -
extern int ext_func(int);
extern int ext_data;
int kext_start(void) { return ext_func(ext_data) + ext_func(2); }
EOF

$mold -arch $ARCH -kext $t/a.o -o $t/kext
cp $t/kext $t/kext0
$mold -arch $ARCH -kext $t/a.o -o $t/kext -kexts_use_stubs

if [ $ARCH = arm64 ]; then
  cmp $t/kext $t/kext0
  exit
fi

otool -rv $t/kext0 > $t/log0
grep -q 'True   BRANCH  False     _ext_func' $t/log0
otool -lv $t/kext0 > $t/lc0
not grep -q 'sectname __stubs' $t/lc0

otool -rv $t/kext > $t/log
not grep -q BRANCH $t/log
grep -q 'False ?( 3)  True   UNSIGND False     _ext_func' $t/log
grep -q 'False ?( 3)  True   UNSIGND False     _ext_data' $t/log
otool -tV $t/kext > $t/text
[ "$(grep -c 'callq.*symbol stub for: _ext_func' $t/text)" = 2 ]
