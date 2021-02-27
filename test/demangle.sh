#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc++ -
int foo(int, int);
int main() {
  foo(3, 4);
}
EOF

! clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o 2> $t/log
grep -q 'undefined symbol: .*: _Z3fooii' $t/log

! clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,-demangle 2> $t/log
grep -Pq 'undefined symbol: .*: foo\(int, int\)' $t/log

echo OK
