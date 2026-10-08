#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# With classic dyld info (below the chained-fixups deployment targets),
# ld64 calls imported functions through lazy pointers bound on first
# use: __stubs jumps through __DATA,__la_symbol_ptr, whose slots start
# out pointing at entries in __TEXT,__stub_helper that push the slot's
# lazy-bind record offset and enter dyld_stub_binder (through the GOT)
# with __dyld_private. ld-prime's layout and code sequences, byte for
# byte; -bind_at_load and chained fixups use GOT-based stubs instead.
cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
#include <unistd.h>
int main(int argc, char **argv) { printf("%d\n", getpid() > 0 ? 4 : 0); return 0; }
EOF2
if [ $ARCH = arm64 ]; then classic=11.0; else classic=12.0; fi
$CC --ld-path=$mold -o $t/exe $t/a.o -mmacosx-version-min=$classic
$RUN $t/exe | grep '^4$'
otool -l $t/exe > $t/lc
grep -q 'LC_DYLD_INFO_ONLY' $t/lc
grep -q 'sectname __stub_helper' $t/lc
grep -q 'sectname __la_symbol_ptr' $t/lc
[ "$(grep -A4 'sectname __la_symbol_ptr' $t/lc | awk '/size/{print $2}')" = 0x0000000000000010 ]
grep -A12 'LC_DYLD_INFO_ONLY' $t/lc | grep 'lazy_bind_size 32'
dyld_info -fixups $t/exe > $t/fixups
grep -q '__got .* bind .*dyld_stub_binder' $t/fixups
grep -q '__la_symbol_ptr .* lazy-bind .*_printf' $t/fixups
grep -q '__la_symbol_ptr .* lazy-bind .*_getpid' $t/fixups
[ "$(grep -c '__la_symbol_ptr .* rebase' $t/fixups)" = 2 ]
nm -m $t/exe > $t/nm
grep -q 'undefined.*dyld_stub_binder' $t/nm
# The word the stub helper hands dyld_stub_binder has its ld64 name.
grep -q '(__DATA,__data) non-external __dyld_private' $t/nm
# The indirect symbol table lists the stubs, the GOT and the lazy
# pointers (the stubs' symbols again) in the sections' order: the GOT
# in __DATA_CONST before the lazy pointers in __DATA, and all of them
# in __DATA under -no_data_const.
otool -I $t/exe > $t/isyms
grep -q 'Indirect symbols for (__DATA,__la_symbol_ptr) 2 entries' $t/isyms
$CC --ld-path=$mold -o $t/exe_ndc $t/a.o -mmacosx-version-min=$classic -Wl,-no_data_const
$RUN $t/exe_ndc | grep '^4$'
r1() { otool -l $t/$1 | awk -v s=$2 '$1 == "sectname" { n = $2 } $1 == "reserved1" && n == s { print $2 }'; }
[ "$(r1 exe __got)" -lt "$(r1 exe __la_symbol_ptr)" ]
otool -Iv $t/exe_ndc > $t/isyms_ndc
grep -A2 'Indirect symbols for (__DATA,__la_symbol_ptr) 2 entries' $t/isyms_ndc | grep _getpid
grep -A2 'Indirect symbols for (__DATA,__got)' $t/isyms_ndc | grep dyld_stub_binder
datasects() { otool -l $t/$1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s { if ($2 == "__DATA") print s; s = "" }' | sort | tr '\n' ' '; }
[ "$(datasects exe_ndc)" = '__data __got __la_symbol_ptr ' ]
# The helper: header, then one entry per stub.
if [ $ARCH = arm64 ]; then
  [ "$(grep -A4 'sectname __stub_helper' $t/lc | awk '/size/{print $2}')" = 0x0000000000000030 ]
  otool -s __TEXT __stub_helper $t/exe | tail -n +3 | cut -c12- | tr -d '\n' | tr -s ' ' > $t/helper
  grep -q 'a9bf47f0' $t/helper       # stp x16, x17, [sp, #-16]!
  grep -q 'd61f0200 18000050' $t/helper   # br x16; ldr w16, #8
else
  # x86-64 entries are 4-byte aligned: 10 bytes of push and jmp, then
  # 2 zero bytes, so the lazy pointers, which start at the entries, are
  # 12 apart.
  otool -s __TEXT __stub_helper $t/exe | tail -n +3 | cut -c12- | tr -d '\n' | tr -s ' ' > $t/helper
  grep -q '4c 8d 1d' $t/helper        # lea __dyld_private(%rip), %r11
  set -- $(otool -s __DATA __la_symbol_ptr $t/exe | tail -n +3 | head -1 | cut -f2)
  [ $((0x${10}${9} - 0x$2$1)) = 12 ]
fi

$CC --ld-path=$mold -o $t/exe_bal $t/a.o -mmacosx-version-min=$classic -Wl,-bind_at_load
$RUN $t/exe_bal | grep '^4$'
otool -l $t/exe_bal > $t/lc_bal
not grep -q '__la_symbol_ptr' $t/lc_bal
dyld_info -fixups $t/exe_bal | grep '__got .* bind .*_printf'

$CC --ld-path=$mold -o $t/exe_ch $t/a.o -mmacosx-version-min=13.0
$RUN $t/exe_ch | grep '^4$'
otool -l $t/exe_ch > $t/lc_ch
not grep -q '__la_symbol_ptr' $t/lc_ch

# With classic dyld info, a stub bound by weak lookup (operator new's)
# skips the helper, and the others go through it.
if [ $ARCH = x86_64 ]; then
  cat <<EOF | $CXX -o $t/c.o -c -xc++ -
#include <cstdio>
#include <new>
int main() { int *p = new int(4); std::printf("%d\n", *p); delete p; }
EOF
  $CXX --ld-path=$mold -o $t/exe_mix $t/c.o -mmacosx-version-min=$classic
  $RUN $t/exe_mix | grep '^4$'
  otool -l $t/exe_mix > $t/lc_mix
  grep -q '__stub_helper' $t/lc_mix
fi
