#!/bin/bash
. $(dirname $0)/common.inc

echo 'int main() { return 0; }' > $t/a.c

$CC -B. -o $t/exe1 $t/a.c -Wl,-build-id
readelf -n $t/exe1 | grep 'GNU.*0x00000014.*NT_GNU_BUILD_ID'

$CC -B. -o $t/exe2 $t/a.c -Wl,-build-id=uuid
readelf -nW $t/exe2 | grep -E 'Build ID: ............4...[89abcdef]'

$CC -B. -o $t/exe3 $t/a.c -Wl,-build-id=md5
readelf -n $t/exe3 | grep 'GNU.*0x00000010.*NT_GNU_BUILD_ID'

$CC -B. -o $t/exe4 $t/a.c -Wl,-build-id=sha1
readelf -n $t/exe4 | grep 'GNU.*0x00000014.*NT_GNU_BUILD_ID'

$CC -B. -o $t/exe5 $t/a.c -Wl,-build-id=sha256
readelf -n $t/exe5 | grep 'GNU.*0x00000020.*NT_GNU_BUILD_ID'

$CC -B. -o $t/exe6 $t/a.c -Wl,-build-id=fast
readelf -n $t/exe6 | grep 'GNU.*0x00000020.*NT_GNU_BUILD_ID'

$CC -B. -o $t/exe7 $t/a.c -Wl,-build-id=0xdeadbeefdeadbeef
readelf -n $t/exe7 | grep 'Build ID: deadbeefdeadbeef'
