#!/bin/bash
source "$(dirname "$0")"/common.inc

# Under -fatal_warnings a warning is an error: it fails the link, which
# leaves no output. -w drops the warnings, which then fail nothing.
# (ld-prime gives each warning as a warning, links to the end and then
# fails with the output in place, counting the warnings -w hid.)
echo 'int main() {}' | $CC -c -xc - -o $t/a.o
$CC --ld-path=$mold $t/a.o -Wl,-fatal_warnings -o $t/exe

rm -f $t/exe
not $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries,-fatal_warnings -o $t/exe 2> $t/log
grep -q 'ignoring duplicate libraries' $t/log
if is_mold; then
  grep -q 'error: ignoring duplicate libraries' $t/log
  [ ! -e $t/exe ]
fi

$CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries,-fatal_warnings,-w -o $t/exe

# The warnings from checking the options are among them: -no_pie draws
# one on arm64, and on x86-64 when it targets macOS 13 or later.
echo 'int main() {}' | $CC -c -xc - -o $t/b.o -mmacosx-version-min=14.0
$CC --ld-path=$mold $t/b.o -mmacosx-version-min=14.0 -Wl,-fatal_warnings -o $t/exe
not $CC --ld-path=$mold $t/b.o -mmacosx-version-min=14.0 \
  -Wl,-no_pie,-fatal_warnings -o $t/exe 2> $t/log2
grep -q -- '-no_pie' $t/log2

# So are those given as an option is read, whatever the order of the
# options.
not $CC --ld-path=$mold $t/a.o -Wl,-alias_list,$t/nosuch,-fatal_warnings -o $t/exe 2> $t/log4
grep -q 'No such file or directory' $t/log4
if is_mold; then
  $CC --ld-path=$mold $t/b.o -mmacosx-version-min=14.0 \
    -Wl,-no_pie,-fatal_warnings,-w -o $t/exe
  $CC --ld-path=$mold $t/a.o -Wl,-fatal_warnings,-w,-alias_list,$t/nosuch -o $t/exe
fi

# A warning of a -r link fails it the same way.
rm -f $t/c.o
not $mold -r -arch $ARCH -o $t/c.o $t/a.o -fatal_warnings -alias_list $t/nosuch 2> $t/log6
grep -q 'No such file or directory' $t/log6
if is_mold; then
  [ ! -e $t/c.o ]
fi

# $LD_TREAT_WARNINGS_AS_ERRORS does as -fatal_warnings, set to anything
# but 0, even to nothing.
not env LD_TREAT_WARNINGS_AS_ERRORS=1 $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries -o $t/exe 2> /dev/null
not env LD_TREAT_WARNINGS_AS_ERRORS= $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries -o $t/exe 2> /dev/null
LD_TREAT_WARNINGS_AS_ERRORS=0 $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries -o $t/exe 2> /dev/null
