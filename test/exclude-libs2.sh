#!/bin/bash
set -e
mold=$1
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -x assembler -c -o $t/a.o -
.globl foo
foo:
  ret
EOF

rm -f $t/b.a
ar crs $t/b.a $t/a.o

cat <<EOF | clang -xc -c -o $t/c.o -
int foo() {
  return 3;
}
EOF

clang -fuse-ld=$mold -shared -o $t/d.so $t/c.o $t/b.a -Wl,-exclude-libs=b.a
readelf --dyn-syms $t/d.so > $t/log
fgrep -q foo $t/log

echo OK
