#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = $(uname -m) ] || skip

echo 'int main() {}' | clang++ -fexperimental-relative-c++-abi-vtables -c \
  -o /dev/null -xc++ - 2> /dev/null || skip

cat <<EOF | clang++ -fexperimental-relative-c++-abi-vtables -c -o $t/a.o -xc++ -fPIC -
#include <stdio.h>

struct Base {
  virtual const char *name() { return "Base"; }
};

struct Derived : Base {
  const char *name() override { return "Derived"; }
};

Base *create() { return new Derived; }
EOF

cat <<EOF | clang++ -fexperimental-relative-c++-abi-vtables -c -o $t/b.o -xc++ -
#include <stdio.h>

struct Base {
  virtual const char *name();
};

Base *create();

int main() {
  Base *p = create();
  printf("%s\n", p->name());
}
EOF

clang++ -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep Derived
