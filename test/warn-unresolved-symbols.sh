#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
int foo();
int main() {
  foo();
}
EOF

! clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o 2>&1 \
  | grep -q 'undefined symbol:.*foo'

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o --warn-unresolved-symbols 2>&1 \
  | grep -q 'undefined symbol:.*foo'

! clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o --warn-unresolved-symbols \
  --error-unresolved-symbols 2>&1 \
  | grep -q 'undefined symbol:.*foo'

echo OK
