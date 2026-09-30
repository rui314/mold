#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void bar() {}
EOF

rm -f $t/lib.a
ar rcs $t/lib.a $t/a.o $t/b.o

cat <<EOF | $CC -o $t/main.o -c -xc -
void foo();
int main() { foo(); }
EOF

# Only a.o loads, and _foo is named as the reason.
$CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a -Wl,-why_load 2> $t/log
grep -q "'_foo' caused load of /.*/lib.a\[2\](a.o)" $t/log
not grep -q '(b\.o)' $t/log

# -u pulls b.o with the forced symbol as the reason.
$CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a -Wl,-why_load -Wl,-u,_bar 2> $t/log2
grep -q "'_bar' caused load of /.*/lib.a\[3\](b.o)" $t/log2

# -all_load loads both, reported as -force_load does. ld-prime names a
# member by the archive's real path and the member's position among
# the archive's entries (the symbol table is [1]), on stderr.
$CC --ld-path=$mold -o $t/exe $t/main.o -Wl,-all_load $t/lib.a -Wl,-why_load 2> $t/log3
grep -q -- '-force_load caused load of /.*/lib.a\[3\](b.o)' $t/log3
