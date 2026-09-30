#!/bin/bash
source "$(dirname "$0")"/common.inc

# -w suppresses warnings. An object built for a newer macOS than the
# link targets draws one from both mold and ld-prime.
cat <<EOF | $CC -o $t/a.o -c -xc - -mmacosx-version-min=15.0
void foo() {}
EOF

$CC --ld-path=$mold -shared -o $t/d.so $t/a.o -mmacosx-version-min=14.0 >& $t/log1

grep -q warning $t/log1

$CC --ld-path=$mold -shared -o $t/d.so $t/a.o -mmacosx-version-min=14.0 -Wl,-w >& $t/log2

not grep -q warning $t/log2

# The warnings from checking the options obey -w too, wherever it
# appears: -no_pie draws one on arm64, and on x86-64 when it targets
# macOS 13 or later.
echo 'int main() { return 0; }' | $CC -o $t/b.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/b.o -mmacosx-version-min=14.0 -Wl,-no_pie >& $t/log3
grep -q warning $t/log3
$CC --ld-path=$mold -o $t/exe $t/b.o -mmacosx-version-min=14.0 -Wl,-no_pie,-w >& $t/log4
not grep -q warning $t/log4

# ld-prime gives some warnings as it reads the option they are about,
# so only a -w before that option silences them: that an -alias_list
# can't be opened, given once, is one, and so are -segprot's and the
# one about an obsolete option, all in command-line order.
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-alias_list,$t/nosuch,-w >& $t/log5
[ "$(grep -c "order file '$t/nosuch' could not be opened" $t/log5)" = 1 ]
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-w,-alias_list,$t/nosuch >& $t/log6
not grep -q warning $t/log6

$CC --ld-path=$mold -o $t/exe $t/b.o \
  -Wl,-segprot,__FOO,rz,r,-no_dead_strip_inits_and_terms,-segprot,__LINKEDIT,r,r,-w >& $t/log7
[ "$(grep -o 'letter .z.\|obsolete\|__LINKEDIT' $t/log7 | tr '\n' ' ')" = \
  "letter 'z' obsolete __LINKEDIT " ]
$CC --ld-path=$mold -o $t/exe $t/b.o \
  -Wl,-w,-segprot,__FOO,rz,r,-no_dead_strip_inits_and_terms,-segprot,__LINKEDIT,r,r >& $t/log8
not grep -q warning $t/log8
