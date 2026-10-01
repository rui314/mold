#!/bin/bash
source "$(dirname "$0")"/common.inc

# A command line that ends before an option's argument is an error that
# names the option and what it needs, as ld-prime words it for each: a
# path, a name, a size, an address, or all of several operands, which
# it takes before it reads any.
echo 'int main() {}' | $CC -c -xc - -o $t/a.o

# missing <message> <options...>
missing() {
  not $mold -arch $ARCH -o $t/exe $t/a.o "${@:2}" 2> $t/log &&
    grep -Fq -- "$1" $t/log
}
missing '-o missing <path>' -o
missing '-L missing <path>' -L
missing '-arch missing <arch>' -arch
missing '-no_allow_dylib_sub_type_mismatches missing <arch_list>' \
  -no_allow_dylib_sub_type_mismatches
missing '-e missing <name>' -e
missing '-headerpad missing <size>' -headerpad
missing '-image_base missing <address>' -image_base
missing '-current_version missing <version>' -current_version
missing '-undefined missing <dynamic_lookup>' -undefined
missing '-mllvm missing <value>' -mllvm
missing '-alias missing <real-name> <alias-name>' -alias _main
missing '-platform_version missing arguments <platform> <min_version> <sdk_version>' \
  -platform_version nosuch 13.0
missing '-sectcreate missing arguments <segname> <sectname> <file>' -sectcreate __TEXT __foo
missing '-add_empty_section missing arguments <segname> <sectname>' -add_empty_section __TEXT
missing '-segaddr needs <segname> <addr>' -segaddr __FOO
missing '-segment_order needs <segment-list>' -segment_order
missing '-sectalign needs <segname> <sectname> <align>' -sectalign __TEXT __text

# Without its path, -rpath is only a warning, and -oso_prefix nothing.
$mold -r -arch $ARCH -o $t/b.o $t/a.o -rpath 2> $t/log
grep -Fq -- 'warning: -rpath missing <path>' $t/log
$mold -r -arch $ARCH -o $t/c.o $t/a.o -oso_prefix

# ld-prime takes an empty argument for a missing one, but for a dylib's
# versions, which it takes for 0, and the few options that don't need
# one. A library option with the name joined to it needs one too.
missing '-e missing <name>' -e ''
missing '-target missing <target-triple>' -target ''
missing '-read_only_relocs missing <option>' -read_only_relocs ''
missing '-platform_version missing arguments <platform> <min_version> <sdk_version>' \
  -platform_version macos '' 13.0
missing '-macos_version_min missing <version>' -macos_version_min ''
missing '-weak-l missing <path>' -weak-l
missing '-needed-l missing <path>' -needed-l
missing '-reexport-l missing <path>' -reexport-l ''
missing '-seg_page_size needs <segname> <size>' -seg_page_size '' 4000
$mold -r -arch $ARCH -o $t/d.o $t/a.o -rpath '' -current_version '' 2> $t/log
grep -Fq -- 'warning: -rpath missing <path>' $t/log

# An architecture ld-prime doesn't know is reported as such.
not $mold -arch foo -o $t/exe $t/a.o 2> $t/log
grep -Fq -- 'unknown -arch name: foo' $t/log
not $mold -target foo-apple-macos14.0 -o $t/exe $t/a.o 2> $t/log
grep -Fq -- "unknown architecture in target triple 'foo-apple-macos14.0'" $t/log
not $mold -arch i386 -o $t/exe $t/a.o 2> $t/log
grep -Fq -- 'linking for i386 is no longer supported' $t/log

# A library option with the name joined to it takes the name as the
# next argument as well, as -l does.
echo 'void foo(void) {}' | $CC -shared -o $t/libfoo.dylib -xc - -Wl,-install_name,@rpath/libfoo.dylib
for opt in -weak-l -needed-l -reexport-l -hidden-l -upward-l -lazy-l; do
  $CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o -L$t -Wl,$opt,foo 2> /dev/null
  otool -L $t/b.dylib | grep -q @rpath/libfoo.dylib
done
