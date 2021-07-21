#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF > $t/a.ver
ver1 {
  global: foo;
  local: *;
};

ver2 {
  global: bar;
};

ver3 {
  global: baz;
};
EOF

cat <<EOF > $t/b.s
.globl foo, bar, baz
foo:
  ret
bar:
  ret
baz:
  ret
EOF

clang -fuse-ld=$mold -shared -o $t/c.so -Wl,-version-script,$t/a.ver $t/b.s

cat <<EOF | clang -xc -c -o $t/d.o -
int foo();
int bar();
int baz();

int main() {
  foo();
  bar();
  baz();
  return 0;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/d.o $t/c.so
$t/exe

readelf --dyn-syms $t/exe > $t/log
fgrep -q 'foo@ver1' $t/log
fgrep -q 'bar@ver2' $t/log
fgrep -q 'baz@ver3' $t/log

echo OK
