#!/bin/bash
source "$(dirname "$0")"/common.inc

# -encryptable (the last of it and -no_encryption) lets the code be
# encrypted after the link: __TEXT's sections start a 16 KiB page of
# their own, but __oslogstring, which goes unencrypted on a page of its
# own at __TEXT's end, and LC_ENCRYPTION_INFO_64 names the pages in
# between, with no encryption system yet (cryptid 0).
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <os/log.h>
int main() { os_log(OS_LOG_DEFAULT, "hello %d", 1); return 0; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-encryptable
otool -l $t/exe > $t/lc
grep -A5 LC_ENCRYPTION_INFO_64 $t/lc > $t/info
grep -q 'cryptoff 16384$' $t/info
grep -q 'cryptsize 16384$' $t/info
grep -q 'cryptid 0$' $t/info
awk '$1 == "cmd" { print $2 }' $t/lc | tr '\n' ' ' > $t/cmds
grep -q 'LC_MAIN LC_ENCRYPTION_INFO_64 LC_LOAD_DYLIB' $t/cmds
grep -A4 'sectname __text' $t/lc | grep -q 'offset 16384$'
grep -A4 'sectname __oslogstring' $t/lc | grep -q 'offset 32768$'
if native_arch; then
  $RUN $t/exe
fi

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-encryptable,-no_encryption
otool -l $t/exe2 > $t/lc2
not grep -q LC_ENCRYPTION_INFO $t/lc2

$mold -r -o $t/b.o $t/a.o -encryptable
otool -l $t/b.o > $t/lc3
not grep -q LC_ENCRYPTION_INFO $t/lc3
