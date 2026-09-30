#!/bin/bash
source "$(dirname "$0")"/common.inc

# A name an export list gives without wildcards is an "initial
# undefine", as a -u name is: it pulls in the archive member defining
# it, keeps its atom under -dead_strip (a hidden one stays, as a
# local), and must resolve - ld-prime reports it even under -undefined
# dynamic_lookup, and so a -u name too. A -r output keeps it as an
# undefined symbol.
cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((visibility("hidden"))) int hidden_fn(void) { return 1; }
int foo = 1;
int main() { return 0; }
EOF
echo 'int fromarchive(void) { return 7; }' | $CC -o $t/b.o -c -xc -
rm -f $t/libb.a
ar rcs $t/libb.a $t/b.o

printf '_foo\n_main\n_fromarchive\n_hidden_fn\n' > $t/list
$CC --ld-path=$mold -o $t/exe $t/a.o $t/libb.a -Wl,-exported_symbols_list,$t/list -Wl,-dead_strip
nm -m $t/exe > $t/nm
grep -Eq 'external _fromarchive$' $t/nm
grep -Eq 'non-external .*_hidden_fn$' $t/nm

printf '_foo\n_nosuch\n' > $t/list2
not $CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-exported_symbols_list,$t/list2 2> $t/log2
grep -q _nosuch $t/log2
not $CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-exported_symbol,_nosuch \
  -Wl,-undefined,dynamic_lookup 2> $t/log3
grep -q _nosuch $t/log3
not $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-u,_nosuch -Wl,-undefined,dynamic_lookup 2> $t/log4
grep -q _nosuch $t/log4

# A pattern is no initial undefine.
printf '_foo\n_nosuch*\n' > $t/list5
$CC --ld-path=$mold -o $t/exe5 $t/a.o -Wl,-exported_symbols_list,$t/list5

$mold -arch $ARCH -r $t/a.o -exported_symbols_list $t/list2 -o $t/r.o
nm -m $t/r.o | grep -q '(undefined) external _nosuch'
