#!/bin/bash
set -e
mold=$1
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fPIC -shared -o $t/a.so -xc -
extern _Thread_local int foo;
_Thread_local int bar;

int get_foo1() { return foo; }
int get_bar1() { return bar; }
EOF

cat <<EOF | cc -c -o $t/b.o -xc -
#include <stdio.h>

_Thread_local int foo;
extern _Thread_local int bar;

int get_foo1();
int get_bar1();

int get_foo2() { return foo; }
int get_bar2() { return bar; }

int main() {
  foo = 5;
  bar = 3;
  printf("%d %d %d %d %d %d\n",
         foo, bar,
         get_foo1(), get_bar1(),
         get_foo2(), get_bar2());
  return 0;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.so $t/b.o
$t/exe | grep -q '5 3 5 3 5 3'

echo OK
