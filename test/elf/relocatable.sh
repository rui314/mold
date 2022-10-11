#!/bin/bash
. $(dirname $0)/common.inc

# https://gcc.gnu.org/bugzilla/show_bug.cgi?id=98667
[ $MACHINE = i386 ] && skip

# We need to merge .opd instead of creating multiple .opd sections
# in an output. That's not implemented yet.
[ $MACHINE = ppc64 ] && skip

cat <<EOF | $CXX -c -o $t/a.o -xc++ -
int one() { return 1; }

struct Foo {
  int three() { static int x = 3; return x++; }
};

int a() {
  Foo x;
  return x.three();
}
EOF

cat <<EOF | $CXX -c -o $t/b.o -xc++ -
int two() { return 2; }

struct Foo {
  int three() { static int x = 3; return x++; }
};

int b() {
  Foo x;
  return x.three();
}
EOF

./mold --relocatable -o $t/c.o $t/a.o $t/b.o

[ -f $t/c.o ]
! [ -x t/c.o ] || false

cat <<EOF | $CXX -c -o $t/d.o -xc++ -
#include <iostream>

int one();
int two();

struct Foo {
  int three();
};

int main() {
  Foo x;
  std::cout << one() << " " << two() << " " << x.three() << "\n";
}
EOF

$CXX -B. -o $t/exe $t/c.o $t/d.o
$QEMU $t/exe | grep -q '^1 2 3$'
