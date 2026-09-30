#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int main() {}' | $CC -c -xc - -o $t/a.o
$CC --ld-path=$mold $t/a.o -Wl,-fatal_warnings -o $t/exe

not $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries,-fatal_warnings -o $t/exe 2> $t/log
grep -q 'duplicate libraries' $t/log

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
not $CC --ld-path=$mold $t/a.o -Wl,-alias_list,$t/nosuch,-fatal_warnings -o $t/exe 2> $t/log3
grep -q "order file '$t/nosuch' could not be opened" $t/log3
