#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

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

# So do the warnings given as the options are read, as ld-prime gives
# them, which only a -w before the option silences: that an -alias_list
# can't be opened is one, and so are -segprot's and the one about an
# obsolete option.
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-alias_list,$t/nosuch >& $t/log5
grep -q 'No such file or directory' $t/log5
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-w,-alias_list,$t/nosuch >& $t/log6
not grep -q warning $t/log6

$CC --ld-path=$mold -o $t/exe $t/b.o \
  -Wl,-segprot,__FOO,rz,r,-no_dead_strip_inits_and_terms,-segprot,__LINKEDIT,r,r >& $t/log7
grep -q "letter 'z'" $t/log7
grep -q obsolete $t/log7
grep -q __LINKEDIT $t/log7
$CC --ld-path=$mold -o $t/exe $t/b.o \
  -Wl,-w,-segprot,__FOO,rz,r,-no_dead_strip_inits_and_terms,-segprot,__LINKEDIT,r,r >& $t/log8
not grep -q warning $t/log8
