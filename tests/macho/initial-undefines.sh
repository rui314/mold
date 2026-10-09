#!/bin/bash
source "$(dirname "$0")"/common.inc

# The names the command line insists on must resolve, even under
# -undefined dynamic_lookup: an undefined -u name, entry point or -alias
# base is wanted by "the command line". The undefined symbols are
# reported by name. (ld-prime says its "<initial-undefines>" wants a -u
# name or entry point, and the alias in its "command-line-aliases-file"
# an -alias base, unless -dead_strip strips the alias.)
cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
extern int zzz_data;
int get(void) { return zzz_data; }
EOF

# Each link here has one undefined symbol: mold names its place on the
# symbol's line, ld-prime on the next. (The logs hold the shell's trace
# of the command too.)
wanted_by() { grep -v '^+' $1 | grep -A1 -- "$2" | grep -qF -- "$3"; }

not $CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-e,_nosuch 2> $t/log1
wanted_by $t/log1 _nosuch 'the command line'

not $CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-e,_nosuch -Wl,-undefined,dynamic_lookup \
  2> $t/log2
wanted_by $t/log2 _nosuch 'the command line'

not $CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-u,_nosuch 2> $t/log3
wanted_by $t/log3 _nosuch 'the command line'

not $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-alias,_nosuch,_alias \
  -Wl,-undefined,dynamic_lookup 2> $t/log4
wanted_by $t/log4 _nosuch 'the command line'
grep -v '^+' $t/log4 > $t/log4.out
not grep -q '"_alias"\|: _alias$' $t/log4.out

not $CC --ld-path=$mold -o $t/exe5 $t/a.o -Wl,-alias,_nosuch,_alias -Wl,-dead_strip 2> $t/log5
wanted_by $t/log5 _nosuch 'the command line'

not $CC --ld-path=$mold -o $t/exe6 $t/a.o -Wl,-alias,_nosuch,_alias -Wl,-dead_strip \
  -Wl,-export_dynamic 2> $t/log6
wanted_by $t/log6 _nosuch 'the command line'

not $CC --ld-path=$mold -o $t/exe7 $t/a.o $t/b.o -Wl,-u,_mmm -Wl,-alias,_ccc,_x \
  -Wl,-e,_bbb 2> $t/log7
[ "$(grep -v '^+' $t/log7 | grep -oE '_(bbb|ccc|mmm|zzz_data)' | uniq | tr '\n' ' ')" = \
  '_bbb _ccc _mmm _zzz_data ' ]

# So the entry point can't be left to dynamic lookup.
not $CC --ld-path=$mold -o $t/exe8 $t/a.o -Wl,-U,_main 2> $t/log8
grep -q "_main is an entry point and can't be used with -U for dynamic lookup" $t/log8
$CC --ld-path=$mold -o $t/lib.dylib -shared $t/b.o -Wl,-U,_zzz_data
