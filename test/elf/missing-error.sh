#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
int foo();

int main() {
  foo();
}
EOF

! $mold -o $t/exe $t/a.o 2> $t/log || false
grep -q 'undefined symbol: .*\.o: foo' $t/log

echo OK
