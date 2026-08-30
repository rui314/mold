#!/usr/bin/env bash
. $(dirname $0)/common.inc

# OneTBB isn't tsan-clean
nm mold | grep '__tsan_init' && skip

[ "$QEMU" == '' ] || skip

# Currently, CREL is not supported on REL-type targets
[[ $MACHINE = arm* ]] && skip
[ $MACHINE = i686 ] && skip

clang -c -xc -o /dev/null /dev/null -Wa,--crel,--allow-experimental-crel || skip

cat <<EOF | clang -o $t/a.o -c -xc++ - -Wa,--crel,--allow-experimental-crel
void hello();

template <typename T>
struct Foo {
  Foo() { hello(); }
};

void baz() { Foo<int> foo; }
EOF

cat <<EOF | clang -o $t/b.o -c -xc++ - -Wa,--crel,--allow-experimental-crel
#include <cstdio>

void hello() { printf("Hello world\n"); }
void baz();

int main() { baz(); }
EOF

./mold -r -o $t/c.o $t/a.o $t/b.o

# The comdat group for Foo's constructor should contain the
# constructor's section along with its relocation section.
readelf --section-groups $t/c.o | grep '\.rela\.text\._ZN3Foo'

clang -B. -o $t/exe $t/c.o
$t/exe | grep 'Hello world'
