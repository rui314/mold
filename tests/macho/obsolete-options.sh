#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime ignores the options of ld64 and older linkers that no longer
# mean anything, with a warning for each, which a -w anywhere silences.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc - -mmacosx-version-min=14.0
link() {
  $mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 14.0 14.0} -syslibroot "$SDK" -lSystem $t/a.o \
    -o $t/exe "$@"
}

link
cp $t/exe $t/exe0

for opt in -allow_simulator_linking_to_macosx_dylibs -b -keep_dwarf_unwind -m -M -new_linker \
  -no_arch_warnings -no_keep_dwarf_unwind -no_kext_objects -no_new_main -nomultidefs -objc_gc \
  -objc_gc_compaction -objc_gc_only -prebind -single_module -Sp -twolevel_namespace_hints -X \
  -s -Si -Sn; do
  link $opt 2> $t/log
  grep -q -- "^[a-z]*: warning: $opt is obsolete$" $t/log
  cmp $t/exe $t/exe0
done

# These take an argument, which may be empty but not missing.
for opt in -kext_objects_dir -multiply_defined -sdk_version -seg_addr_table -Y; do
  link $opt suppress 2> $t/log
  grep -q -- "warning: $opt is obsolete$" $t/log
  cmp $t/exe $t/exe0
  link $opt '' 2> $t/log
  grep -q -- "warning: $opt is obsolete$" $t/log
  not link $opt 2> $t/log
  grep -q -- "$opt.*argument" $t/log
done

link -X -s -multiply_defined suppress -segprot __FOO rz r -Si -b 2> $t/log
[ "$(grep -o -- "-[A-Za-z_]* is obsolete\|letter 'z'" $t/log | sort -u | tr '\n' ' ')" = \
  "-Si is obsolete -X is obsolete -b is obsolete -multiply_defined is obsolete -s is obsolete letter 'z' " ]

# A -w before them silences them.
link -w -X -s -Si 2> $t/log
not grep -q "is obsolete" $t/log

not link -fatal_warnings -X -s 2> $t/log
grep -q -- '-X is obsolete' $t/log
