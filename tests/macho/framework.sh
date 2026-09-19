#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <CoreFoundation/CoreFoundation.h>
int main() {
  CFStringRef s = CFSTR("hello");
  printf("%ld\n", (long)CFStringGetLength(s));
}
EOF2

$CC --ld-path=$mold -framework CoreFoundation -o $t/exe $t/a.o
$t/exe | grep '^5$'
