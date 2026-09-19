#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
static void helper_one() {}
static void helper_two() {}
static void keep_me() {}
void (*keep1)(void) = helper_one;
void (*keep2)(void) = helper_two;
void (*keep3)(void) = keep_me;
int main() {}
EOF

# Default: all three locals are in the symbol table.
$CC --ld-path=$mold -o $t/exe $t/a.o
nm $t/exe > $t/nm0
grep -q _helper_one $t/nm0 && grep -q _keep_me $t/nm0

# Strip list: matching locals disappear, globals are untouched.
cat <<EOF > $t/strip
_helper_*
EOF
$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-non_global_symbols_strip_list,$t/strip
nm $t/exe1 > $t/nm1
! grep -q _helper_one $t/nm1 || false
! grep -q _helper_two $t/nm1 || false
grep -q _keep_me $t/nm1
grep -q ' _main$' $t/nm1

# Keep list: only matching locals stay.
cat <<EOF > $t/keep
_keep_me
EOF
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-non_global_symbols_keep_list,$t/keep
nm $t/exe2 > $t/nm2
! grep -q _helper_one $t/nm2 || false
grep -q _keep_me $t/nm2
grep -q ' _main$' $t/nm2
