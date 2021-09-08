#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
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

cat <<EOF | clang -fuse-ld=$mold -xc -shared -o $t/b.so -Wl,-version-script,$t/a.ver -
void foo() {}
void bar() {}
void baz() {}
EOF

cat <<EOF | clang -xc -c -o $t/c.o -
void foo();
void bar();
void baz();

int main() {
  foo();
  bar();
  baz();
  return 0;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.o $t/b.so
$t/exe

readelf --dyn-syms $t/exe > $t/log
fgrep -q 'foo@ver1' $t/log
fgrep -q 'bar@ver2' $t/log
fgrep -q 'baz@ver3' $t/log

echo OK
