#!/bin/bash
. $(dirname $0)/common.inc

# ld-prime skips an empty argument, which names no file, on the command
# line or in a response file.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/a.o -Xlinker ''
$RUN $t/exe

echo "'' \"\"" > $t/rsp
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,@$t/rsp
$RUN $t/exe
