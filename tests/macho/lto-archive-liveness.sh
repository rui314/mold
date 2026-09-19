#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -flto -c -xc - -o $t/unused.o
#include <stdio.h>
__attribute__((constructor)) static void init() { puts("constructor"); }
int unused() { return 42; }
EOF
echo 'int used() { return 3; }' | $CC -flto -c -xc - -o $t/used.o
echo 'int main() { return 0; }' | $CC -c -xc - -o $t/empty.o
rm -f $t/libunused.a $t/libmixed.a
ar rcs $t/libunused.a $t/unused.o
ar rcs $t/libmixed.a $t/used.o $t/unused.o

# No live bitcode at all: the archive's constructor must stay out.
$CC --ld-path=$mold $t/empty.o $t/libunused.a -o $t/empty
$t/empty > $t/log
test ! -s $t/log

cat <<EOF | $CC -c -xc - -o $t/main.o
#include <stdio.h>
int used();
int main() { printf("%d\n", used()); }
EOF
$CC --ld-path=$mold $t/main.o $t/libmixed.a -o $t/mixed
$t/mixed > $t/log
test "$(cat $t/log)" = 3

# Explicitly loading every member must still keep its initializer.
$CC --ld-path=$mold $t/main.o -Wl,-force_load,$t/libmixed.a -o $t/forced
$t/forced > $t/log
printf 'constructor\n3\n' > $t/expected
cmp $t/log $t/expected
