#!/bin/bash
source "$(dirname "$0")"/common.inc

# A command line that ends before an option's argument, or one of its
# operands, is an error that names the option.
echo 'int main() {}' | $CC -c -xc - -o $t/a.o

# missing <option> <options...>
missing() {
  not $mold -arch $ARCH -o $t/exe $t/a.o "${@:2}" 2> $t/log &&
    grep -q -- "$1.*missing" $t/log
}
missing -o -o
missing -L -L
missing -arch -arch
missing -no_allow_dylib_sub_type_mismatches -no_allow_dylib_sub_type_mismatches
missing -e -e
missing -headerpad -headerpad
missing -image_base -image_base
missing -current_version -current_version
missing -undefined -undefined
missing -mllvm -mllvm
missing -alias -alias _main
missing -platform_version -platform_version macos 13.0
missing -sectcreate -sectcreate __TEXT __foo
missing -add_empty_section -add_empty_section __TEXT

# Without its path, -rpath is only a warning, and -oso_prefix nothing.
$mold -r -arch $ARCH -o $t/b.o $t/a.o -rpath 2> $t/log
grep -q -- 'warning: -rpath missing <path>' $t/log
$mold -r -arch $ARCH -o $t/c.o $t/a.o -oso_prefix

# ld-prime takes an empty argument for a missing one, but for a dylib's
# versions, which it takes for 0, and the few options that don't need
# one. A library option with the name joined to it needs one too.
missing -e -e ''
missing -target -target ''
missing -read_only_relocs -read_only_relocs ''
missing -platform_version -platform_version macos '' 13.0
missing -macos_version_min -macos_version_min ''
missing -weak-l -weak-l
missing -needed-l -needed-l
missing -reexport-l -reexport-l ''
not $mold -arch $ARCH -o $t/exe $t/a.o -seg_page_size '' 4000 2> $t/log
grep -q -- -seg_page_size $t/log
not $mold -arch $ARCH -o $t/exe $t/a.o -segaddr __FOO 2> $t/log
grep -q -- -segaddr $t/log
not $mold -arch $ARCH -o $t/exe $t/a.o -segment_order 2> $t/log
grep -q -- -segment_order $t/log
not $mold -arch $ARCH -o $t/exe $t/a.o -sectalign __TEXT __text 2> $t/log
grep -q -- -sectalign $t/log
$mold -r -arch $ARCH -o $t/d.o $t/a.o -rpath '' -current_version '' 2> $t/log
grep -q -- 'warning: -rpath missing <path>' $t/log

# It takes an option's name for -rpath's path, with the warning, and
# the image gets no such run path.
$CC --ld-path=$mold -shared -o $t/e.dylib $t/a.o -Wl,-rpath,-dead_strip 2> $t/log
grep -q -- 'warning: -rpath missing <path>' $t/log
otool -l $t/e.dylib > $t/load
not grep -F LC_RPATH $t/load

# An architecture mold doesn't link for is refused, by -arch or a
# target triple.
not $mold -arch foo -o $t/exe $t/a.o 2> $t/log
grep -Fq -- 'unknown -arch name: foo' $t/log
not $mold -target foo-apple-macos14.0 -o $t/exe $t/a.o 2> $t/log
grep -Fq -- "unknown architecture in target triple 'foo-apple-macos14.0'" $t/log
not $mold -arch i386 -o $t/exe $t/a.o 2> $t/log
grep -q i386 $t/log

# A library option with the name joined to it takes the name as the
# next argument as well, as -l does.
echo 'void foo(void) {}' | $CC -shared -o $t/libfoo.dylib -xc - -Wl,-install_name,@rpath/libfoo.dylib
for opt in -weak-l -needed-l -reexport-l -hidden-l -upward-l -lazy-l; do
  $CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o -L$t -Wl,$opt,foo 2> /dev/null
  otool -L $t/b.dylib | grep -q @rpath/libfoo.dylib
done
