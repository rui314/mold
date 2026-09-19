#!/bin/bash
source "$(dirname "$0")"/common.inc

# An entry point defined in a dylib: app extensions are linked with
# -e _NSExtensionMain, which lives in Foundation and which nothing in
# the extension references. The symbol must still be claimed from the
# dylib, and LC_MAIN, which must point into __TEXT, names its stub.
# libSystem's _exit stands in here: dyld passes argc as the first
# argument, so the exit status is the argument count.
cat <<EOF | $CC -o $t/a.o -c -xc -
int unused = 1;
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-e,_exit
st=0; $t/exe || st=$?
[ $st = 1 ]
st=0; $t/exe a b || st=$?
[ $st = 3 ]

nm -m $t/exe | grep -q 'undefined.*_exit'
otool -l $t/exe | grep -q LC_MAIN

# A -r link has no entry point: its output must not acquire an
# undefined _main (a dylib built from Xcode's prelinked package
# object then failed with "undefined symbol: _main").
$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm $t/r.o > $t/nm-r
not grep -q ' U _main' $t/nm-r
$CC --ld-path=$mold -shared -o $t/libr.dylib $t/r.o
