#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64's -force_flat_namespace made an executable bind its dylibs'
# imports flat as well (MH_FORCE_FLAT). ld-prime takes it for
# -flat_namespace, with a warning; a later -twolevel_namespace undoes
# it.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("Hello\n"); }
EOF
link() { $mold -arch $ARCH -syslibroot "$SDK" -lSystem $t/a.o -o $t/exe "$@"; }

link -flat_namespace
cp $t/exe $t/flat
link
cp $t/exe $t/twolevel

link -force_flat_namespace 2> $t/log
grep -q -- 'warning: -force_flat_namespace is no longer supported, using -flat_namespace instead' \
  $t/log
cmp $t/exe $t/flat
$RUN $t/exe | grep -q Hello

link -w -force_flat_namespace 2> $t/log
not grep -q -- 'warning: -force_flat_namespace' $t/log

link -force_flat_namespace -twolevel_namespace 2> /dev/null
cmp $t/exe $t/twolevel
