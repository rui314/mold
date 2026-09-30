#!/bin/bash
source "$(dirname "$0")"/common.inc

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
$t/exe | grep '^4$'
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
# The indirect symbol table lists the stubs, the GOT, then the lazy
# pointers (the stubs' symbols again): the sections' order, which
# -no_data_const changes, the GOT then following the lazy pointers and
# the input sections in __DATA.
otool -I $t/exe > $t/isyms
grep -q 'Indirect symbols for (__DATA,__la_symbol_ptr) 2 entries' $t/isyms
$CC --ld-path=$mold -o $t/exe_ndc $t/a.o -mmacosx-version-min=$classic -Wl,-no_data_const
r1() { otool -l $t/$1 | awk -v s=$2 '$1 == "sectname" { n = $2 } $1 == "reserved1" && n == s { print $2 }'; }
[ "$(r1 exe __got)" -lt "$(r1 exe __la_symbol_ptr)" ]
[ "$(r1 exe_ndc __la_symbol_ptr)" -lt "$(r1 exe_ndc __got)" ]
datasects() { otool -l $t/$1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s { if ($2 == "__DATA") printf "%s ", s; s = "" }'; }
[ "$(datasects exe_ndc)" = '__la_symbol_ptr __data __got ' ]
# The helper: header, then one entry per stub.
if [ $ARCH = arm64 ]; then
  [ "$(grep -A4 'sectname __stub_helper' $t/lc | awk '/size/{print $2}')" = 0x0000000000000030 ]
  otool -s __TEXT __stub_helper $t/exe | tail -n +3 | cut -c12- | tr -d '\n' | tr -s ' ' > $t/helper
  grep -q 'a9bf47f0' $t/helper       # stp x16, x17, [sp, #-16]!
  grep -q 'd61f0200 18000050' $t/helper   # br x16; ldr w16, #8
else
  # ld-prime keeps x86-64 entries 4-byte aligned: 10 bytes of push and
  # jmp, then 2 zero bytes before the next entry (none after the last),
  # so the lazy pointers, which start at the entries, are 12 apart.
  [ "$(grep -A4 'sectname __stub_helper' $t/lc | awk '/size/{print $2}')" = 0x0000000000000026 ]
  otool -s __TEXT __stub_helper $t/exe | tail -n +3 | cut -c12- | tr -d '\n' | tr -s ' ' > $t/helper
  grep -q '4c 8d 1d' $t/helper        # lea __dyld_private(%rip), %r11
  set -- $(otool -s __DATA __la_symbol_ptr $t/exe | tail -n +3 | head -1 | cut -f2)
  [ $((0x${10}${9} - 0x$2$1)) = 12 ]
fi

$CC --ld-path=$mold -o $t/exe_bal $t/a.o -mmacosx-version-min=$classic -Wl,-bind_at_load
$t/exe_bal | grep '^4$'
otool -l $t/exe_bal > $t/lc_bal
not grep -q '__la_symbol_ptr' $t/lc_bal
dyld_info -fixups $t/exe_bal | grep '__got .* bind .*_printf'

$CC --ld-path=$mold -o $t/exe_ch $t/a.o -mmacosx-version-min=13.0
$t/exe_ch | grep '^4$'
otool -l $t/exe_ch > $t/lc_ch
not grep -q '__la_symbol_ptr' $t/lc_ch

# x86-64 stubs are byte-aligned with classic dyld info, 2-byte aligned
# with chained fixups - and with classic dyld info too once one stub
# (operator new's, bound by weak lookup) skips the helper.
if [ $ARCH = x86_64 ]; then
  grep -A8 'sectname __stubs' $t/lc | grep 'align 2^0'
  grep -A8 'sectname __stubs' $t/lc_ch | grep 'align 2^1'
  cat <<EOF | $CXX -o $t/c.o -c -xc++ -
#include <cstdio>
#include <new>
int main() { int *p = new int(4); std::printf("%d\n", *p); delete p; }
EOF
  $CXX --ld-path=$mold -o $t/exe_mix $t/c.o -mmacosx-version-min=$classic
  $t/exe_mix | grep '^4$'
  otool -l $t/exe_mix > $t/lc_mix
  grep -q '__stub_helper' $t/lc_mix
  grep -A8 'sectname __stubs' $t/lc_mix | grep 'align 2^1'
fi
