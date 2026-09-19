#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
const char *a() { return "duplicated string"; }
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
const char *a();
const char *b() { return "duplicated string"; }
int main() {
  printf("%d %s\n", a() == b(), a());
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep '^1 duplicated string$'

# The string should appear only once in the output
[ "$(strings $t/exe | grep -c 'duplicated string')" = 1 ]
