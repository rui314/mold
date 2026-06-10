#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 3; }
EOF

rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/a.o

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() {}
EOF

# Without EXTERN, the archive member is not pulled out
$CC -B. -o $t/exe1 $t/b.o $t/libfoo.a
not grep -qw foo <(readelf -sW $t/exe1)

# EXTERN(foo) is equivalent to -u foo
echo 'EXTERN(foo)' > $t/script
$CC -B. -o $t/exe2 $t/b.o $t/libfoo.a -Wl,-T,$t/script
readelf -sW $t/exe2 | grep -w foo
