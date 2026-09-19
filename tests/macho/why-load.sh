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
$CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a -Wl,-why_load > $t/log
grep -q '_foo forced load of .*lib.a(a.o)' $t/log
! grep -q 'b.o' $t/log || false

# -u pulls b.o with the forced symbol as the reason.
$CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a -Wl,-why_load -Wl,-u,_bar > $t/log2
grep -q '_bar forced load of .*lib.a(b.o)' $t/log2

# -all_load loads both, reported as option-forced.
$CC --ld-path=$mold -o $t/exe $t/main.o -Wl,-all_load $t/lib.a -Wl,-why_load > $t/log3
grep -q 'forced load of .*lib.a(b.o)' $t/log3
