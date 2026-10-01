#!/bin/bash
source "$(dirname "$0")"/common.inc

# In a -r output, which a relocation may need them in, ld-prime keeps
# the local symbols -x drops from a final image, and those a
# -non_global_symbols_(no_)strip_list strips, by names it makes up:
# l<nnn>, numbered in symbol table order by the counter that names
# literals (LC<n>). -x, which strip -x passes, also drops the debug
# notes and the N_AST paths. Under a list the notes stay, naming a
# renamed local by its new name, a demoted private external by its own.
cat <<EOF | $CC -g -o $t/a.o -c -xc -
static int s1(void) { return 1; }
static int s2(void) { return 2; }
static int sd = 5;
__attribute__((visibility("hidden"))) int hid(void) { return 7; }
int f(void) { return s1() + s2() + sd + hid(); }
EOF

$mold -arch $ARCH -r -x -o $t/r1.o $t/a.o -add_ast_path /x/a.swiftmodule
nm -ap $t/r1.o > $t/log1
grep -q ' t l001$' $t/log1
grep -q ' t l002$' $t/log1
grep -q ' t l003$' $t/log1
grep -q ' d l004$' $t/log1
grep -q ' T _f$' $t/log1
not grep -q -e _s1 -e _sd -e _hid -e ' SO ' -e swiftmodule $t/log1

# The demoted private external is not stripped under
# -keep_private_externs.
$mold -arch $ARCH -r -x -keep_private_externs -o $t/r2.o $t/a.o
nm -ap $t/r2.o > $t/log2
grep -q ' T _hid$' $t/log2
grep -q ' d l003$' $t/log2

echo _s1 > $t/strip.txt
$mold -arch $ARCH -r -non_global_symbols_strip_list $t/strip.txt -o $t/r3.o $t/a.o
nm -ap $t/r3.o > $t/log3
grep -q ' t l001$' $t/log3
grep -q ' FUN l001$' $t/log3
grep -q ' t _s2$' $t/log3
grep -q ' FUN _s2$' $t/log3

$mold -arch $ARCH -r -non_global_symbols_no_strip_list $t/strip.txt -o $t/r4.o $t/a.o
nm -ap $t/r4.o > $t/log4
grep -q ' t _s1$' $t/log4
not grep -q ' t _s2$' $t/log4
not grep -q ' t _hid$' $t/log4
grep -q ' FUN _hid$' $t/log4
grep -q ' STSYM l[0-9][0-9][0-9]$' $t/log4
