#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF > $t/a.c
#include <stdio.h>
static void hello() { printf("Hello world\n"); }
__attribute__((visibility("hidden"))) void hidden() { hello(); }
int main(){ hidden(); }
EOF

$CC -o $t/a.o -c $t/a.c

$CC --ld-path=$mold -o $t/exe1 $t/a.o
nm $t/exe1 > $t/log1
grep -qw _hello $t/log1
grep -qw _hidden $t/log1

# -x drops every local symbol, the private externals it demotes too.
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-x
nm $t/exe2 > $t/log2
not grep -qw _hello $t/log2
not grep -qw _hidden $t/log2
