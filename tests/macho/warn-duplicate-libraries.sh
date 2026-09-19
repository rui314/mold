#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF
rm -f $t/libfoo.a; ar rcs $t/libfoo.a $t/a.o

cat <<EOF | $CC -o $t/main.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -lfoo -lfoo 2> $t/log
grep -q "ignoring duplicate libraries: '-lfoo'" $t/log

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -lfoo -lfoo \
  -Wl,-no_warn_duplicate_libraries 2> $t/log2
! grep -q 'duplicate libraries' $t/log2 || false
