#!/bin/bash
source "$(dirname "$0")"/common.inc

# The options and the list files that name symbols name them in bytes,
# UTF-8 or not, as the symbols' names are (see symbol-name-bytes): -e,
# -u, -U, -alias, the export lists, -alias_list, -order_file and
# -why_live alike. A diagnostic prints them with a U+FFFD for each byte
# that isn't UTF-8.

cat <<'EOF' | $CC -o $t/a.o -c -xc -
int foo(void) __asm__("_f\377o");
int foo(void) { return 3; }
int bar(void) { return 4; }
int baz(void) __asm__("_b\377z");
int baz(void) { return 5; }
EOF

cat <<'EOF' | $CC -o $t/main.o -c -xc -
int foo(void) __asm__("_f\377o");
int main() { return foo(); }
EOF

cat <<'EOF' | $CC -o $t/entry.o -c -xc -
int start(void) __asm__("_st\377rt");
int start(void) { return 7; }
EOF

$CC --ld-path=$mold -o $t/exe $t/entry.o -Wl,-e,$'_st\xffrt'
$RUN $t/exe || [ $? = 7 ]

not $CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o -Wl,-u,$'_x\xff' 2> $t/log2
grep -q $'_x\xef\xbf\xbd' $t/log2

$CC --ld-path=$mold -o $t/exe3 $t/main.o -Wl,-U,$'_f\xffo'
nm -m $t/exe3 > $t/nm3
grep -q $'(undefined) external _f\xffo (dynamically looked up)' $t/nm3

# An export list's names and patterns.
printf '_f\377o\n# a comment\n_b\377*\n' > $t/exports
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -Wl,-exported_symbols_list,$t/exports
nm -gU $t/libfoo.dylib > $t/nm4
grep -q $' T _f\xffo$' $t/nm4
grep -q $' T _b\xffz$' $t/nm4
not grep -q ' T _bar$' $t/nm4

$CC --ld-path=$mold -shared -o $t/libfoo2.dylib $t/a.o -Wl,-exported_symbol,$'_b\xff*'
nm -gU $t/libfoo2.dylib > $t/nm5
grep -q $' T _b\xffz$' $t/nm5
not grep -q $' T _f\xffo$' $t/nm5

# Aliases.
$CC --ld-path=$mold -o $t/exe6 $t/main.o $t/a.o -Wl,-alias,$'_f\xffo',$'_al\xffias'
nm $t/exe6 > $t/nm6
grep -q $' T _al\xffias$' $t/nm6

printf '_f\377o _al\377ias2\n' > $t/aliases
$CC --ld-path=$mold -o $t/exe7 $t/main.o $t/a.o -Wl,-alias_list,$t/aliases
nm $t/exe7 > $t/nm7
grep -q $' T _al\xffias2$' $t/nm7

# An order file, and the entry naming nothing it reports.
printf '_b\377z\n_f\377o\n_bar\n_n\377ne\n' > $t/order
$CC --ld-path=$mold -o $t/exe8 $t/main.o $t/a.o -Wl,-order_file,$t/order \
  -Wl,-order_file_statistics 2> $t/log8
grep -q $'order_file entry: _n\xef\xbf\xbdne$' $t/log8
nm -n $t/exe8 | grep ' T ' > $t/nm8
grep -A1 $' T _b\xffz$' $t/nm8 | grep -q $' T _f\xffo$'
grep -A1 $' T _f\xffo$' $t/nm8 | grep -q ' T _bar$'

# -why_live.
$CC --ld-path=$mold -o $t/exe9 $t/main.o $t/a.o -Wl,-dead_strip -Wl,-why_live,$'_f\xffo' 2> $t/log9
grep -q $'^_f\xef\xbf\xbdo from ' $t/log9
