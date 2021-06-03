#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

echo 'int main() { return 0; }' > $t/a.c

clang -o $t/exe $t/a.c -fuse-ld=`pwd`/../mold -Wl,-build-id
readelf -n $t/exe | grep -q 'GNU.*0x00000010.*NT_GNU_BUILD_ID'

clang -o $t/exe $t/a.c -fuse-ld=`pwd`/../mold -Wl,-build-id=uuid
readelf -nW $t/exe |
  grep -Pq 'GNU.*0x00000010.*NT_GNU_BUILD_ID.*Build ID: ............4...[89abcdef]'

clang -o $t/exe $t/a.c -fuse-ld=`pwd`/../mold -Wl,-build-id=md5
readelf -n $t/exe | grep -q 'GNU.*0x00000010.*NT_GNU_BUILD_ID'

clang -o $t/exe $t/a.c -fuse-ld=`pwd`/../mold -Wl,-build-id=sha1
readelf -n $t/exe | grep -q 'GNU.*0x00000014.*NT_GNU_BUILD_ID'

clang -o $t/exe $t/a.c -fuse-ld=`pwd`/../mold -Wl,-build-id=sha256
readelf -n $t/exe | grep -q 'GNU.*0x00000020.*NT_GNU_BUILD_ID'

clang -o $t/exe $t/a.c -fuse-ld=`pwd`/../mold -Wl,-build-id=0xdeadbeef
readelf -n $t/exe | grep -q 'Build ID: deadbeef'

echo OK
