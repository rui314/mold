#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc++ -
int foo(int, int);
int main() {
  foo(3, 4);
}
EOF

if clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,-no-demangle 2> $t/log; then false; fi
grep -q 'undefined symbol: .*: _Z3fooii' $t/log

if clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,-demangle 2> $t/log; then false; fi
grep -Pq 'undefined symbol: .*: foo\(int, int\)' $t/log

if clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o 2> $t/log; then false; fi
grep -Pq 'undefined symbol: .*: foo\(int, int\)' $t/log

cat <<EOF | clang -c -o $t/b.o -xc -
extern int Pi;
int main() {
  return Pi;
}
EOF

if clang -fuse-ld=`pwd`/../mold -o $t/exe $t/b.o -Wl,-demangle 2> $t/log; then false; fi
grep -q 'undefined symbol: .*: Pi' $t/log

echo OK
