#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF > $t/a.cc
int main() {
  try {
    throw 0;
  } catch (int x) {
    return x;
  }
  return 1;
}
EOF

clang++ -fuse-ld=`pwd`/../mold -o $t/exe $t/a.cc -static
$t/exe

clang++ -fuse-ld=`pwd`/../mold -o $t/exe $t/a.cc
$t/exe

echo ' OK'
