#!/bin/bash
source "$(dirname "$0")"/common.inc

# Objects whose __objc_imageinfo give different Swift ABI versions (the
# second byte of the flags) don't link together: ld-prime names the
# first version it saw and the one that differs. With
# $LD_WARN_ON_SWIFT_ABI_VERSION_MISMATCHES set, it warns of each and
# keeps the first.
mk() {
  {
    echo '.section __DATA,__objc_imageinfo,regular,no_dead_strip'
    echo '.long 0'
    echo ".long $(( ($2 << 8) | 64 ))"
    [ -z "$3" ] || printf '.globl _main\n.text\n_main: ret\n'
  } | $CC -o $t/$1.o -c -xassembler -
}
mk v5 5 main
mk v6 6
mk v8 8
dir=$(cd $t && pwd -P)

not $CC --ld-path=$mold -o $t/exe $t/v5.o $t/v6.o 2> $t/log
grep -q "not all .o files built with the same Swift ABI version. Started with (4.0), now found (4.1/4.2) in $dir/v6.o" $t/log

LD_WARN_ON_SWIFT_ABI_VERSION_MISMATCHES=1 $CC --ld-path=$mold -o $t/exe \
  $t/v5.o $t/v6.o $t/v8.o 2> $t/log
grep -q "warning: $dir/v6.o compiled with a different Swift ABI version (4.1/4.2), than previous files (4.0)" $t/log
grep -q "warning: $dir/v8.o compiled with a different Swift ABI version (unknown ABI version 0x08), than previous files (4.0)" $t/log
otool -s __DATA_CONST __objc_imageinfo $t/exe | grep -Eq '00000000 00000540|00 00 00 00 40 05 00 00'
