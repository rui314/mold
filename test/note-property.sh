#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fcf-protection=branch -c -o $t/a.o -xc -
int main() {
  return 0;
}
EOF

cat <<EOF | clang -fcf-protection=none -c -o $t/b.o -xc -
int main() {
  return 0;
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o
readelf -n $t/exe | grep -q 'x86 feature: IBT'

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/b.o
! readelf -n $t/exe | grep -q 'x86 feature: IBT' || false

echo OK
