#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime reports duplicate definitions only once dead stripping is
# done - and only of the symbols it leaves live - and none if a symbol
# is undefined.
cat <<EOF | $CC -o $t/a.o -c -xc -
int dup = 1;
int nosuch(void);
int main() { return nosuch(); }
EOF
echo 'int dup = 2;' | $CC -o $t/b.o -c -xc -

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o 2> $t/log
grep -v '^+' $t/log > $t/msgs
grep -qi 'undefined symbol' $t/msgs
grep -q _nosuch $t/msgs
not grep -q 'duplicate symbol' $t/msgs

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-undefined,dynamic_lookup 2> $t/log
grep -q 'duplicate symbol.*_dup' $t/log

cat <<EOF | $CC -o $t/c.o -c -xc -
int dup = 1;
int main() { return 0; }
EOF
$CC --ld-path=$mold -o $t/exe $t/c.o $t/b.o -Wl,-dead_strip
not $CC --ld-path=$mold -o $t/exe $t/c.o $t/b.o -Wl,-dead_strip -Wl,-u,_dup 2> $t/log
grep -q 'duplicate symbol.*_dup' $t/log
