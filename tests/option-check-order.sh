#!/bin/bash
source "$(dirname "$0")"/common.inc

# Once it has read the options, ld-prime checks them in an order of its
# own, warning and failing as it goes: a fatal error leaves the checks
# after it - and their warnings - out. The warnings about obsolete
# options come almost last, after the deprecated -force_symbols_*_list
# (which /usr/lib's libraries still use), and only the one about an
# unused -e comes after them.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
echo _main > $t/list
sdk=$(xcrun --show-sdk-path)
link() {
  $mold -arch $ARCH -platform_version macos 26.0 26.0 -syslibroot "$sdk" -lSystem $t/a.o \
    -o $t/exe "$@" 2> $t/log
}
warnings() { grep -o 'warning: .*' $t/log | sed 's/^warning: //' > $t/got; }
[ $ARCH = arm64 ] && arm64=1 || arm64=

link -mark_dead_strippable_dylib -force_symbols_weak_list $t/list -U _x -undefined dynamic_lookup \
  -read_only_relocs suppress -headerpad 0x10 -image_base 0x100001001 -stack_size 0 \
  -pagezero_size 0x1001 -seg_page_size __DATA 0x5000 -segalign 0x5000 -no_pie \
  -segaddr __FOO 0x200000000 -segaddr __FOO 0x200000000
warnings
{
  echo '-segaddr __FOO used more than once'
  echo '-no_pie is deprecated when targeting new OS versions'
  [ $arm64 ] && echo '-no_pie ignored for arm64*'
  echo 'alignment for -segalign 0x5000 is not a power of two, using 0x4000'
  echo '-seg_page_size for __DATA is not a power of two, rounding down to 0x4000'
  echo '-pagezero_size not aligned, rounded up to: 0x4000, use -segalign to change the alignment'
  echo '-stack_size 0x0 has no effect'
  echo 'base address 0x100001001 is not properly aligned. Changing it to 0x100004000'
  [ $arm64 ] && echo 'Linking with PIE, -image_base will be ignored'
  echo '-headerpad 0x10 is too small, at least 32 bytes are required to reserve space for code signature'
  echo '-read_only_relocs relocs cannot be used in this configuration'
  echo '-U option is redundant when using -undefined dynamic_lookup'
  echo '-force_symbols_[not_]weak_list is deprecated'
  echo '-mark_dead_strippable_dylib is obsolete'
} | diff - $t/got

link -dylib -install_name /usr/lib/libfoo.dylib -mark_dead_strippable_dylib -e _main \
  -force_symbols_weak_list $t/list -headerpad 0x10 -segalign 0x5000 -rpath /x -pie
warnings
{
  echo '-pie being ignored. It is only used when linking a main executable'
  echo 'OS dylibs should not add rpaths (linker option: -rpath) (Xcode build setting: LD_RUNPATH_SEARCH_PATHS)'
  echo 'alignment for -segalign 0x5000 is not a power of two, using 0x4000'
  echo '-headerpad 0x10 is too small, at least 32 bytes are required to reserve space for code signature'
  echo '-mark_dead_strippable_dylib is obsolete'
  echo 'ignoring -e, not used for output type'
} | diff - $t/got

# A fatal error stops the checks there.
not link -mark_dead_strippable_dylib -segalign 0x5000 -no_pie -kernel
not grep -q warning $t/log
grep -q -- '-kernel must be used with -static' $t/log
not link -mark_dead_strippable_dylib -segalign 0x5000 -no_pie -make_mergeable
warnings
not grep -q 'segalign\|obsolete' $t/got
grep -q -- '-no_pie is deprecated' $t/got
not link -mark_dead_strippable_dylib -segalign 0x5000 -dylib -pagezero_size 0x1000
warnings
[ "$(cat $t/got)" = 'alignment for -segalign 0x5000 is not a power of two, using 0x4000' ]
not link -mark_dead_strippable_dylib -force_symbols_weak_list $t/list -headerpad 0x10 \
  -reexported_symbols_list $t/list
warnings
not grep -q 'obsolete\|deprecated' $t/got
grep -q 'headerpad' $t/got

# With -undefined dynamic_lookup, -U is ignored: it may name the entry
# point then.
not link -e _main -U _main
grep -q "_main is an entry point and can't be used with -U for dynamic lookup" $t/log
link -e _main -U _main -undefined dynamic_lookup
grep -q -- '-U option is redundant' $t/log
