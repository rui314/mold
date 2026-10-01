#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 could still link the fragile (version 1) Objective-C ABI of
# 32-bit macOS on request. ld-prime knows the modern one alone: it takes
# -objc_abi_version 2, spelled just so, and refuses anything else as it
# reads it.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
sdk=$(xcrun --show-sdk-path)
link() { $mold -arch $ARCH -syslibroot "$sdk" -lSystem $t/a.o -o $t/exe "$@"; }

link
cp $t/exe $t/exe0
link -objc_abi_version 2
cmp $t/exe $t/exe0

for v in 1 02 2.0 3 ' 2'; do
  not link -objc_abi_version "$v" 2> $t/log
  grep -q -- "-objc_abi_version '$v' not supported (expected 2)" $t/log
done

not link -objc_abi_version '' 2> $t/log
grep -q -- '-objc_abi_version missing <version>' $t/log
not link -objc_abi_version 2> $t/log
grep -q -- '-objc_abi_version missing <version>' $t/log

not link -segprot __FOO rz r -objc_abi_version 1 -foo 2> $t/log
grep -q "unknown -segprot letter 'z'" $t/log
grep -q -- "-objc_abi_version '1' not supported" $t/log
not grep -q 'unknown options' $t/log
