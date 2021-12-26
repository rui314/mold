#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF > $t/a.ver
ver1 {
  global: ?oo;
  local: *;
};

ver2 {
  global: b?r;
};
EOF

cat <<EOF | clang -fuse-ld=$mold -xc -shared -o $t/b.so -Wl,-version-script,$t/a.ver -
void foo() {}
void bar() {}
void baz() {}
EOF

cat <<EOF | clang -xc -c -o $t/c.o -
void foo();
void bar();

int main() {
  foo();
  bar();
  return 0;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.o $t/b.so
$t/exe

readelf --dyn-syms $t/b.so > $t/log
fgrep -q 'foo@@ver1' $t/log
fgrep -q 'bar@@ver2' $t/log
! fgrep -q 'baz' $t/log || false

echo OK
