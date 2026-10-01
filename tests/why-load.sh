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

# Resolution runs again after LTO: the members loaded before keep their
# reasons, a bitcode one LTO compiled included.
echo 'int nat(void); int bc(void); int main() { return nat() + bc(); }' |
  $CC -flto -o $t/lto-main.o -c -xc -
echo 'int nat(void) { return 1; }' | $CC -o $t/nat.o -c -xc -
echo 'int bc(void) { return 2; }' | $CC -flto -o $t/bc.o -c -xc -
rm -f $t/libnat.a $t/libbc.a
ar rcs $t/libnat.a $t/nat.o
ar rcs $t/libbc.a $t/bc.o
$CC --ld-path=$mold -flto -o $t/exe $t/lto-main.o $t/libnat.a $t/libbc.a -Wl,-why_load 2> $t/log4
grep -q "'_nat' caused load of /.*/libnat.a\[2\](nat.o)" $t/log4
grep -q "'_bc' caused load of /.*/libbc.a\[2\](bc.o)" $t/log4

# ld-prime says so as it resolves the symbols, before it checks the
# inputs' versions, in a -r link too.
cat <<EOF2 | $CC -o $t/new.o -c -xc - -mmacosx-version-min=99.0
void foo();
int main() { foo(); }
EOF2
$CC --ld-path=$mold -o $t/exe $t/new.o $t/lib.a -Wl,-why_load 2> $t/log5
grep -n "caused load of /.*/lib.a\[2\](a.o)" $t/log5 | cut -d: -f1 > $t/line5
grep -n 'was built for newer' $t/log5 | cut -d: -f1 >> $t/line5
[ "$(grep -c . $t/line5)" = 2 ]
[ "$(sort -n $t/line5)" = "$(cat $t/line5)" ]
$mold -r -arch $ARCH -o $t/r.o $t/main.o $t/lib.a -why_load 2> $t/log6
grep -q "'_foo' caused load of /.*/lib.a\[2\](a.o)" $t/log6
