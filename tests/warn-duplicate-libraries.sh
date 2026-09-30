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

# One warning lists every option given more than once, sorted, each as
# spelled; the same library under another option is no repeat.
cat <<EOF | $CC -o $t/b.o -c -xc -
void bar() {}
EOF
rm -f $t/libbar.a; ar rcs $t/libbar.a $t/b.o
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -lfoo -lbar -lfoo -lbar -lfoo 2> $t/log3
[ "$(grep -c 'duplicate libraries' $t/log3)" = 1 ]
grep -q "ignoring duplicate libraries: '-lbar', '-lfoo'" $t/log3

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -lfoo -Wl,-hidden-lfoo 2> $t/log4
not grep -q 'duplicate libraries' $t/log4
