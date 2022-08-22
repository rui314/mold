#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$testname
mkdir -p $t

cat <<'EOF' | cc -o $t/a.o -c -xc -
#include <stdio.h>
#include <stdint.h>

extern char a __asm("section$start$__TEXT$__text");
extern char b __asm("section$end$__TEXT$__text");

extern char c __asm("section$start$__TEXT$__foo");
extern char d __asm("section$end$__TEXT$__foo");

extern char e __asm("section$start$__FOO$__foo");
extern char f __asm("section$end$__FOO$__foo");

extern char g __asm("segment$start$__TEXT");
extern char h __asm("segment$end$__TEXT");

int main() {
  printf("%p %p %p %p %p %p %p %p\n", &a, &b, &c, &d, &e, &f, &g, &h);
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o
$t/exe > /dev/null

echo OK
