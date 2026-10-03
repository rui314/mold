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
grep -q -- '-mcpu.*missing' $t/log

# The ThinLTO cache: ld-prime creates its directory (one level, owner
# only) and hands libLTO the policy, decimal numbers, a pruning interval
# of -1 never pruning. It warns, and goes without, if it can't. Merged
# modules have no cache.
link() { $CC -flto --ld-path=$mold -o $t/exe $t/a.o $t/b.o "$@"; }
rm -rf $t/cache
link -Wl,-cache_path_lto,$t/cache,-arch_variant_lto_cache_mismatch,suppress
[ ! -e $t/cache ]
$CC -flto=thin -o $t/c.o -c -xc - <<< 'int times2(int x) { return x * 2; }'
$CC -flto=thin -o $t/d.o -c -xc - \
  <<< '#include <stdio.h>
int times2(int);
int main() { printf("%d\n", times2(21)); }'
thinlink() { $CC -flto=thin --ld-path=$mold -o $t/exe $t/c.o $t/d.o "$@"; }
thinlink -Wl,-cache_path_lto,$t/cache,-prune_interval_lto,-1,-prune_after_lto,3600 \
  -Wl,-max_relative_cache_size_lto,50
$t/exe | grep -q '^42$'
[ "$(stat -f %Lp $t/cache)" = 700 ]
ls $t/cache | grep -q '^llvmcache-'
thinlink -Wl,-cache_path_lto,$t/cache
$t/exe | grep -q '^42$'
thinlink -Wl,-cache_path_lto,$t/exe 2> $t/log
grep -q "warning: unable to create ThinLTO cache directory: $t/exe (17)" $t/log
# -mllvm options go to libLTO, which parses them as LLVM's command line
# (clang passes one of its own): an unknown one ends the link.
not link -Wl,-mllvm,-bogus-option 2> $t/log
grep -q "Unknown command line argument '-bogus-option'" $t/log
link -Wl,-mllvm,-inline-threshold=100
$t/exe | grep -q '^42$'

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
