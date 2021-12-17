#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
void expfn1() {}
void expfn2() {}
int main() {}
EOF

cat <<EOF | cc -shared -o $t/b.so -xc -
void expfn1();
void expfn2() {}

int foo() {
  expfn1();
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.so
readelf --dyn-syms $t/exe | grep -q expfn2
readelf --dyn-syms $t/exe | grep -q expfn1

echo OK
