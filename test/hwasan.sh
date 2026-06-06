#!/bin/bash
. $(dirname $0)/common.inc

# Verify that mold can link object files compiled with Clang's HWASAN
# instrumentation.

[ "$CC" = cc ] || skip

# Skip unless Clang can build a HWASAN executable on this machine (e.g.
# the sanitizer runtime may not be installed). We probe with Clang's
# default linker so that a failure to link with mold below is reported as
# a test failure rather than silently skipped.
echo 'int main() {}' | clang -fsanitize=hwaddress -o /dev/null -xc - 2> /dev/null || skip

cat <<EOF | clang -fsanitize=hwaddress -c -o $t/a.o -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

clang -fsanitize=hwaddress -B. -o $t/exe $t/a.o
