#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $GCC -c -o $t/a.o -xc -
#include <stdio.h>

__attribute__((section(".gnu.warning.foo")))
static const char foo[] = "foo is deprecated";

__attribute__((section(".gnu.warning.bar")))
const char bar[] = "bar is deprecated";

int main() {
  printf("Hello world\n");
}
EOF

# Make sure that we do not copy .gnu.warning.* sections.
$CC -B. -o $t/exe $t/a.o -no-pie
$QEMU $t/exe | grep 'Hello world'
