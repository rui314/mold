#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fPIC -c -o $t/a.o -xc -
void foo1() {}
void foo2() {}
void foo3() {}

__asm__(".symver foo1, foo@VER1");
__asm__(".symver foo2, foo@VER2");
__asm__(".symver foo3, foo@@VER3");

void foo();

void bar() {
  foo();
}
EOF

echo 'VER1 { local: *; }; VER2 { local: *; }; VER3 { local: *; };' > $t/b.ver
clang -fuse-ld=$mold -shared -o $t/c.so $t/a.o -Wl,--version-script=$t/b.ver
readelf --symbols $t/c.so > $t/log

fgrep -q 'foo@VER1' $t/log
fgrep -q 'foo@VER2' $t/log
fgrep -q 'foo@@VER3' $t/log

echo OK
