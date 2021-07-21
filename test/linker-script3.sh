#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

mkdir -p $t/foo

cat <<EOF | cc -o $t/foo/a.o -c -xc -
int main() {}
EOF

cat <<EOF > $t/b.script
INPUT(a.o)
EOF

clang -o $t/exe -L$t/foo $t/b.script

echo OK
