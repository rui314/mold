#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
static void foo() {}
void bar() {}
void baz() {}
int main() {}
EOF

cat <<EOF > $t/symbols
foo
baz
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o \
  -Wl,--retain-symbols-file=$t/symbols
readelf --symbols $t/exe > $t/log

! grep -qw foo $t/log || false
! grep -qw bar $t/log || false
! grep -qw main $t/log || false

grep -qw baz $t/log

echo OK
