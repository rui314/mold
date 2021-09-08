#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -xc -
int foo();
int main() { foo(); }
EOF

cmd="clang -fuse-ld=$mold -o $t/exe $t/a.o"

! $cmd 2>&1 | grep -q 'undefined.*foo'
! $cmd -Wl,-unresolved-symbols=report-all 2>&1 | grep -q 'undefined.*foo'

$cmd -Wl,-unresolved-symbols=ignore-all

! readelf --dyn-syms $t/exe | grep -w foo || false

$cmd -Wl,-unresolved-symbols=report-all -Wl,--warn-unresolved-symbols 2>&1 | \
  grep -q 'undefined.*foo'

! $cmd -Wl,-unresolved-symbols=ignore-in-object-files 2>&1 | grep -q 'undefined.*foo'
! $cmd -Wl,-unresolved-symbols=ignore-in-shared-libs 2>&1 | grep -q 'undefined.*foo'

echo OK
