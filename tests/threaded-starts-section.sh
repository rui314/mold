#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 chained a static arm64e image's rebases through its pointers from
# a __TEXT,__thread_starts list. ld-prime has chained fixups instead and
# refuses the option.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
link() { $mold -arch $ARCH -o $t/exe $t/a.o "$@"; }

for opts in '-static' '-fixup_chains' '-no_fixup_chains -w' '-r'; do
  not link -threaded_starts_section $opts 2> $t/log
  grep -q -- -threaded_starts_section $t/log
done
