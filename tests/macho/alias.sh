#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void real_impl() { printf("hello\n"); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void official_name();
int main() { official_name(); }
EOF

# _official_name resolves to the same code as _real_impl.
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-alias,_real_impl,_official_name
$t/exe | grep -q hello
nm $t/exe > $t/nm
[ "$(grep ' _real_impl$' $t/nm | awk '{print $1}')" = \
  "$(grep ' _official_name$' $t/nm | awk '{print $1}')" ]

# The same through a list file, with comments.
cat <<EOF > $t/aliases
# compatibility names
_real_impl _official_name
EOF
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -Wl,-alias_list,$t/aliases
$t/exe2 | grep -q hello

# An alias of nothing is an error.
not $CC --ld-path=$mold -o $t/exe3 $t/a.o $t/b.o -Wl,-alias,_nonexistent,_official_name 2> $t/log
grep -q 'undefined base symbol' $t/log
