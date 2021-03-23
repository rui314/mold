#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -fno-PIE -
extern int foo;

int main() {
  return foo;
}
EOF

cat <<EOF | cc -shared -o $t/b.so -xc -
__attribute__((visibility("protected"))) int foo;
EOF

! clang -fuse-ld=`pwd`/../mold $t/a.o $t/b.so -o $t/exe >& $t/log
fgrep -q 'cannot make copy relocation for  protected symbol' $t/log

echo OK
