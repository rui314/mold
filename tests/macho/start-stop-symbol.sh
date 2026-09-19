#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<'EOF' | $CC -o $t/a.o -c -xc -
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

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe > /dev/null
