#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -xc -
void bar();
void foo() {
  bar();
}
EOF

cat <<EOF | cc -c -o $t/b.o -xc -
void bar() {}
EOF

cat <<EOF | cc -c -o $t/c.o -xc -
void baz() {}
EOF

cat <<EOF | cc -c -o $t/d.o -xc -
void foo();
int main() {
  foo();
}
EOF

ar crs $t/e.a $t/a.o $t/b.o $t/c.o
$mold -r -o $t/f.o $t/d.o $t/e.a

readelf --symbols $t/f.o > $t/log
grep -q 'foo$' $t/log
grep -q 'bar$' $t/log
! grep -q 'baz$' $t/log || false

echo OK
