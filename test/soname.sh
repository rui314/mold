#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fPIC -c -o $t/a.o -xc -
int foo() {
  return 3;
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/b.so -shared $t/a.o -Wl,-soname,foo
readelf --dynamic $t/b.so > $t/log
fgrep -q 'Library soname: [foo]' $t/log

echo OK
