#!/bin/bash
source "$(dirname "$0")"/common.inc

# The options that tune LTO. -save-temps keeps the merged bitcode, before
# and after optimization, and the object libLTO compiled it to beside the
# output, as ld64 named them; -mcpu has libLTO compile for that CPU.
cat <<EOF | $CC -flto -o $t/a.o -c -xc -
int times2(int x) { return x * 2; }
EOF
cat <<EOF | $CC -flto -o $t/b.o -c -xc -
#include <stdio.h>
int times2(int);
int main() { printf("%d\n", times2(21)); }
EOF

rm -f $t/exe.lto.*
$CC -flto --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-save-temps
$t/exe | grep -q '^42$'
[ -s $t/exe.lto.bc ]
[ -s $t/exe.lto.opt.bc ]
otool -hv $t/exe.lto.o | grep -q OBJECT

echo 'int main() { return 0; }' | $CC -o $t/c.o -c -xc -
$CC --ld-path=$mold -o $t/exe2 $t/c.o -Wl,-save-temps
[ ! -e $t/exe2.lto.o ]

$CC -flto --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-mcpu,bogus 2> $t/log
grep -q "'bogus' is not a recognized processor for this target" $t/log
not $mold -arch $ARCH -o $t/exe $t/a.o $t/b.o -mcpu 2> $t/log
grep -q -- '-mcpu missing <cpu>' $t/log

# mold compiles all bitcode as one module, as ld-prime does but for
# ThinLTO: it has no ThinLTO cache, and takes the options that tune one
# with their arguments' checks only.
link() { $CC -flto --ld-path=$mold -o $t/exe $t/a.o $t/b.o "$@"; }
link -Wl,-cache_path_lto,$t/cache,-prune_interval_lto,10,-prune_after_lto,3600 \
  -Wl,-max_relative_cache_size_lto,100,-arch_variant_lto_cache_mismatch,suppress
for opt in -prune_interval_lto -prune_after_lto -max_relative_cache_size_lto; do
  not link -Wl,$opt,0x10 2> $t/log
  grep -q "invalid argument for $opt" $t/log
done
not link -Wl,-max_relative_cache_size_lto,101 2> $t/log
grep -q 'Expect a value between 0 and 100 for -max_relative_cache_size_lto' $t/log
not link -Wl,-arch_variant_lto_cache_mismatch,ignore 2> $t/log
grep -q -- '-arch_variant_lto_cache_mismatch invalid option (warning | error | suppress)' $t/log

for opt in -no_lto_softload_runtime_symbols -lto_softload_runtime_symbols \
  -use_lto_filenames_in_order_file_matching -no_use_lto_filenames_in_order_file_matching; do
  link -Wl,$opt
  $t/exe | grep -q '^42$'
done
