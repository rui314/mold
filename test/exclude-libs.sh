#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -x assembler -c -o $t/a.o -
.globl foo
foo:
  ret
EOF

cat <<EOF | clang -x assembler -c -o $t/b.o -
.globl bar
bar:
  ret
EOF

rm -f $t/c.a
ar crs $t/c.a $t/a.o

rm -f $t/d.a
ar crs $t/d.a $t/b.o

cat <<EOF | clang -xc -c -o $t/e.o -
int foo();
int bar();

int baz() {
  foo();
  bar();
  return 0;
}
EOF

clang -fuse-ld=$mold -shared -o $t/f.so $t/e.o $t/c.a $t/d.a
readelf --dyn-syms $t/f.so > $t/log
fgrep -q foo $t/log
fgrep -q bar $t/log
fgrep -q baz $t/log

clang -fuse-ld=$mold -shared -o $t/f.so $t/e.o $t/c.a $t/d.a -Wl,-exclude-libs=c.a
readelf --dyn-syms $t/f.so > $t/log
! fgrep -q foo $t/log || false
fgrep -q bar $t/log
fgrep -q baz $t/log

clang -fuse-ld=$mold -shared -o $t/f.so $t/e.o $t/c.a $t/d.a -Wl,-exclude-libs=c.a -Wl,-exclude-libs=d.a
readelf --dyn-syms $t/f.so > $t/log
! fgrep -q foo $t/log || false
! fgrep -q bar $t/log || false
fgrep -q baz $t/log

clang -fuse-ld=$mold -shared -o $t/f.so $t/e.o $t/c.a $t/d.a -Wl,-exclude-libs=ALL
readelf --dyn-syms $t/f.so > $t/log
! fgrep -q foo $t/log || false
! fgrep -q bar $t/log || false
fgrep -q baz $t/log

echo OK
