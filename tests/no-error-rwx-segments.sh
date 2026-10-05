#!/usr/bin/env bash
. $(dirname $0)/common.inc

# GNU ld 2.39 and later warn about RWX segments and executable stacks,
# and these options turn the warnings or errors off. mold never reports
# RWX segments and never makes an executable stack an error, so it
# accepts the options as no-ops.

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--no-warn-rwx-segments \
  -Wl,--no-error-rwx-segments -Wl,--no-error-execstack
$QEMU $t/exe | grep -q 'Hello world'
