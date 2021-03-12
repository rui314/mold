#!/bin/bash
set -e
cd $(dirname $0)
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

clang -fuse-ld=`pwd`/../mold $t/a.o $t/b.so -o $t/exe
readelf -a $t/exe > $t/log

fgrep -q '[24] .dynbss' $t/log
fgrep -q '[25] .dynbss.rel.ro' $t/log
fgrep -q '0000000000202200   100 OBJECT  GLOBAL DEFAULT   24 readwrite' $t/log
fgrep -q '0000000000202280   100 OBJECT  GLOBAL DEFAULT   25 readonly' $t/log

echo OK
