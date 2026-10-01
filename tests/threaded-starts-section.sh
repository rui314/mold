#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 chained a static arm64e image's rebases through its pointers from
# a __TEXT,__thread_starts list. ld-prime has chained fixups instead and
# refuses the option, with -fixup_chains in other words, once it has
# read every option and checked some: after the -kernel and
# -bundle_loader checks and the lazy-load warnings, before the -r
# -dead_strip one and the obsolete options' warnings.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
link() { $mold -arch $ARCH -o $t/exe $t/a.o "$@"; }

not link -threaded_starts_section -static 2> $t/log
grep -q -- '-threaded_starts_section is no longer supported$' $t/log

not link -threaded_starts_section -fixup_chains 2> $t/log
grep -q -- "-fixup_chains\*, -rebase_section and -threaded_starts_section can't be used together" \
  $t/log

not link -threaded_starts_section -no_fixup_chains -w 2> $t/log
grep -q -- '-threaded_starts_section is no longer supported$' $t/log

not link -threaded_starts_section -kernel 2> $t/log
grep -q -- '-kernel must be used with -static' $t/log

not link -threaded_starts_section -X -r -dead_strip 2> $t/log
grep -q -- '-threaded_starts_section is no longer supported$' $t/log
not grep -q -- '-dead_strip cannot\|-X is obsolete' $t/log

not link -threaded_starts_section -lazy-lfoo -platform_version macos 14.0 14.0 2> $t/log
grep -q "lazy-load will be ignored for 'foo'" $t/log
grep -q -- '-threaded_starts_section is no longer supported$' $t/log

not link -threaded_starts_section -foo 2> $t/log
grep -q 'unknown options: -foo' $t/log
not grep -q -- '-threaded_starts_section is' $t/log
