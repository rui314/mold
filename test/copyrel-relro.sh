#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -fno-PIE -
extern const char readonly[100];
extern char readwrite[100];

int main() {
  return readonly[0] + readwrite[0];
}
EOF

cat <<EOF | cc -shared -o $t/b.so -xc -
const char readonly[100] = "abc";
char readwrite[100] = "abc";
EOF

clang -fuse-ld=$mold $t/a.o $t/b.so -o $t/exe
readelf -a $t/exe > $t/log

grep -Pqz '(?s)\[(\d+)\] .dynbss.rel.ro .* \1 readonly' $t/log
grep -Pqz '(?s)\[(\d+)\] .dynbss .* \1 readwrite' $t/log

echo OK
