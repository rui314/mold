#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF > $t/a.ver
{
  extern "C" { foo };
  local: *;
};
EOF

cat <<EOF | c++ -fPIC -c -o $t/b.o -xc -
int foo = 5;
int main() { return 0; }
EOF

clang -fuse-ld=$mold -shared -o $t/c.so -Wl,-version-script,$t/a.ver $t/b.o

readelf --dyn-syms $t/c.so > $t/log
fgrep -q foo $t/log
! fgrep -q ' main' $t/log || false

echo OK
