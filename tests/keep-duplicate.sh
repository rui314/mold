#!/bin/bash
source "$(dirname "$0")"/common.inc

# -keep_duplicate and -keep_duplicates_list (wildcards allowed) name the
# functions, local ones too, that function deduplication neither folds
# nor folds others into.
cat <<EOF | $CC -o $t/a.o -c -xc - -O1
#define F(f) __attribute__((noinline, visibility("hidden"))) int f(int x) { return x * 3 + 7; }
F(f1) F(f2) F(f3)
__attribute__((noinline)) static int f4(int x) { return x * 3 + 7; }
int main(int argc, char **argv) { return f1(argc) + f2(argc) + f3(argc) + f4(argc); }
EOF

addr() { nm $1 > $1.nm; awk "/ _$2\$/ { print \$1 }" $1.nm; }

# (x86-64's last function, the static f4, lacks the others' padding and
# folds with none.)
$CC -O2 --ld-path=$mold -o $t/exe1 $t/a.o
[ "$(addr $t/exe1 f1)" = "$(addr $t/exe1 f3)" ]

$CC -O2 --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-keep_duplicate,_f1
[ "$(addr $t/exe2 f1)" != "$(addr $t/exe2 f2)" ]
[ "$(addr $t/exe2 f2)" = "$(addr $t/exe2 f3)" ]

echo '_f[12]' > $t/list
$CC -O2 --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-keep_duplicates_list,$t/list
[ "$(addr $t/exe3 f1)" != "$(addr $t/exe3 f2)" ]
[ "$(addr $t/exe3 f2)" != "$(addr $t/exe3 f3)" ]

if [ $ARCH = arm64 ]; then
  [ "$(addr $t/exe1 f1)" = "$(addr $t/exe1 f4)" ]
  $CC -O2 --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-keep_duplicate,_f4
  [ "$(addr $t/exe4 f1)" = "$(addr $t/exe4 f3)" ]
  [ "$(addr $t/exe4 f1)" != "$(addr $t/exe4 f4)" ]
fi

not $mold -o $t/exe5 $t/a.o -keep_duplicate 2> $t/log
grep -q -- '-keep_duplicate missing <name>' $t/log
