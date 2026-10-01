#!/bin/bash
source "$(dirname "$0")"/common.inc

# -force_symbols_weak_list and -force_symbols_not_weak_list make the
# exported definitions they name weak or not, which dyld then
# coalesces, or not: the image calls a forced-weak one through a stub.
# ld-prime deprecates both.
cat <<EOF | $CC -o $t/a.o -c -xc - -O0
int sfoo() { return 1; }
__attribute__((weak)) int wfoo() { return 3; }
__attribute__((visibility("hidden"))) int hid() { return 4; }
int caller() { return sfoo() + wfoo() + hid(); }
EOF

echo _sfoo > $t/weak.txt
echo _wfoo > $t/not-weak.txt
$CC --ld-path=$mold -o $t/a.dylib -shared $t/a.o -Wl,-force_symbols_weak_list,$t/weak.txt \
  -Wl,-force_symbols_not_weak_list,$t/not-weak.txt 2> $t/log
grep -q 'warning: -force_symbols_\[not_\]weak_list is deprecated' $t/log
[ "$(grep -c deprecated $t/log)" = 1 ]
dyld_info -exports $t/a.dylib > $t/exports
grep -q '_sfoo \[weak-def\]' $t/exports
grep '_wfoo' $t/exports > $t/wfoo
not grep -q weak-def $t/wfoo
otool -tV $t/a.dylib > $t/text
grep -q 'symbol stub for: _sfoo' $t/text
not grep -q 'symbol stub for: _wfoo' $t/text

# A hidden definition can't be forced.
echo _hid > $t/hid.txt
$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -Wl,-force_symbols_weak_list,$t/hid.txt 2> $t/log
grep -q 'warning: cannot force to be weak, non-external symbol _hid' $t/log

# An object file keeps its definitions as they are.
$mold -r -o $t/c.o $t/a.o -force_symbols_weak_list $t/weak.txt 2> /dev/null
nm -m $t/c.o | grep _sfoo > $t/sfoo
not grep -q weak $t/sfoo

not $mold -o $t/d.dylib -dylib $t/a.o -force_symbols_not_weak_list 2> $t/log
grep -q -- '-force_symbols_weak_list missing <path>' $t/log
