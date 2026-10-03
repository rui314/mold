#!/bin/bash
source "$(dirname "$0")"/common.inc

# Under -fatal_warnings ld-prime still gives each warning as a warning
# and links to the end, then fails with the output in place.
echo 'int main() {}' | $CC -c -xc - -o $t/a.o
$CC --ld-path=$mold $t/a.o -Wl,-fatal_warnings -o $t/exe

not $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries,-fatal_warnings -o $t/exe 2> $t/log
grep -q "warning: ignoring duplicate libraries: '-lSystem'" $t/log
grep -q 'fatal warning(s) induced error (-fatal_warnings)' $t/log

# -w hides that one altogether.
$CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries,-fatal_warnings,-w -o $t/exe

# The warnings from checking the options are among them: -no_pie draws
# one on arm64, and on x86-64 when it targets macOS 13 or later. Hidden
# by -w, it still counts.
echo 'int main() {}' | $CC -c -xc - -o $t/b.o -mmacosx-version-min=14.0
$CC --ld-path=$mold $t/b.o -mmacosx-version-min=14.0 -Wl,-fatal_warnings -o $t/exe
not $CC --ld-path=$mold $t/b.o -mmacosx-version-min=14.0 \
  -Wl,-no_pie,-fatal_warnings -o $t/exe 2> $t/log2
grep -q 'warning: -no_pie' $t/log2
not $CC --ld-path=$mold $t/b.o -mmacosx-version-min=14.0 \
  -Wl,-no_pie,-fatal_warnings,-w -o $t/exe 2> $t/log3
not grep -q 'warning:' $t/log3
grep -q 'fatal warning(s) induced error (-fatal_warnings)' $t/log3

# So are those given as an option is read, whatever the order of the
# options, and one -w hides.
not $CC --ld-path=$mold $t/a.o -Wl,-alias_list,$t/nosuch,-fatal_warnings -o $t/exe 2> $t/log4
grep -q "order file '$t/nosuch' could not be opened" $t/log4
not $CC --ld-path=$mold $t/a.o -Wl,-fatal_warnings,-w,-alias_list,$t/nosuch -o $t/exe 2> $t/log5
not grep -q 'warning:' $t/log5
grep -q 'fatal warning(s) induced error (-fatal_warnings)' $t/log5

# A warning of a -r link fails it the same way. (The compiler driver
# removes the output of a link that fails.)
rm -f $t/c.o
not $mold -r -arch $ARCH -o $t/c.o $t/a.o -fatal_warnings -alias_list $t/nosuch 2> $t/log6
grep -q 'fatal warning(s) induced error (-fatal_warnings)' $t/log6
[ -f $t/c.o ]

# $LD_TREAT_WARNINGS_AS_ERRORS does as -fatal_warnings, set to anything
# but 0, even to nothing.
not env LD_TREAT_WARNINGS_AS_ERRORS=1 $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries -o $t/exe 2> $t/log7
grep -q 'fatal warning(s) induced error (-fatal_warnings)' $t/log7
not env LD_TREAT_WARNINGS_AS_ERRORS= $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries -o $t/exe 2> /dev/null
LD_TREAT_WARNINGS_AS_ERRORS=0 $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries -o $t/exe 2> /dev/null
