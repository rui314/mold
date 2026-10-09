#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime knows one -undefined treatment besides the default error:
# dynamic_lookup, which suppress selects too, silently but for its
# deprecation. It deprecates every other one - error, warning, or
# anything else - and ignores it: -undefined warning still fails the
# link, and no treatment undoes an earlier dynamic_lookup. -U names
# nothing new once dynamic_lookup covers every symbol.
cat <<EOF | $CC -o $t/a.o -c -xc -
int missing(void);
int main() { return missing(); }
EOF

not $CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-undefined,warning 2> $t/log1
grep -q -- '-undefined warning is deprecated' $t/log1
grep -q _missing $t/log1

not $CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-undefined,bogus 2> $t/log2
grep -q -- '-undefined bogus is deprecated' $t/log2

$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-undefined,dynamic_lookup \
  -Wl,-undefined,error 2> $t/log3
grep -q -- '-undefined error is deprecated' $t/log3
nm -m $t/exe3 | grep -q '_missing (dynamically looked up)'

$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-undefined,suppress 2> $t/log4
grep -q -- '-undefined suppress is deprecated' $t/log4
not grep -q _missing $t/log4
nm -m $t/exe4 | grep -q '_missing (dynamically looked up)'

$CC --ld-path=$mold -o $t/exe5 $t/a.o -Wl,-U,_missing -Wl,-U,_other \
  -Wl,-undefined,dynamic_lookup 2> $t/log5
grep -q -- '-U option is redundant when using -undefined dynamic_lookup' $t/log5
[ "$(grep -c -- '-U option is redundant' $t/log5)" = 1 ]

# A kext looks every import up anyway.
cat <<EOF | $CC -o $t/k.o -c -xc -mkernel -
extern int kernel_var;
int kext_start(void) { return kernel_var; }
EOF
$mold -arch $ARCH -kext -U _kernel_var $t/k.o -o $t/kext 2> $t/log8
grep -q -- '-U option is redundant when using -undefined dynamic_lookup' $t/log8

# The warnings obey -fatal_warnings and -w. A treatment is deprecated as
# the option is read, which only a -fatal_warnings or -w before obeys.
not $CC --ld-path=$mold -o $t/exe6 $t/a.o -Wl,-fatal_warnings -Wl,-undefined,dynamic_lookup \
  -Wl,-undefined,error 2> /dev/null
$CC --ld-path=$mold -o $t/exe7 $t/a.o -Wl,-w -Wl,-undefined,suppress -Wl,-U,_missing \
  2> $t/log7
not grep -q warning $t/log7
$CC --ld-path=$mold -o $t/exe7 $t/a.o -Wl,-undefined,suppress -Wl,-U,_missing -Wl,-w \
  2> $t/log9
not grep -q -- '-U option is redundant' $t/log9
