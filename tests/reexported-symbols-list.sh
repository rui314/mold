#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int foo() { return 42; } int bar() { return 7; }' |
  $CC --ld-path=$mold -dynamiclib -xc - -o $t/libinner.dylib \
    -install_name $PWD/$t/libinner.dylib
echo 'int wrapper() { return 0; }' | $CC -c -xc - -o $t/a.o
echo '_foo # selected export' > $t/list

$CC --ld-path=$mold -dynamiclib $t/a.o $t/libinner.dylib -o $t/libouter.dylib \
  -install_name $PWD/$t/libouter.dylib \
  -Wl,-reexported_symbols_list,$t/list,-dead_strip,-dead_strip_dylibs
dyld_info -exports $t/libouter.dylib > $t/exports
grep -q 're-export.*_foo' $t/exports
grep -q _wrapper $t/exports
not grep -q _bar $t/exports
otool -l $t/libouter.dylib > $t/loads
not grep -q LC_REEXPORT_DYLIB $t/loads

echo 'int foo(); int main() { return foo() != 42; }' | $CC -c -xc - -o $t/main.o
$CC --ld-path=$mold $t/main.o $t/libouter.dylib -o $t/exe
$t/exe

# A listed name is an initial undefine, as a -u name is: one nothing
# defines is wanted by the command line, even under -undefined
# dynamic_lookup. (ld-prime names its "<initial-undefines>", on the line
# after the symbol's.)
echo _missing > $t/list
not $CC --ld-path=$mold -dynamiclib $t/a.o $t/libinner.dylib \
  -Wl,-reexported_symbols_list,$t/list,-dead_strip -o $t/libbad.dylib 2> $t/log
grep -v '^+' $t/log | grep -A1 _missing | grep -qF 'the command line'
not $CC --ld-path=$mold -dynamiclib $t/a.o $t/libinner.dylib -Wl,-undefined,dynamic_lookup \
  -Wl,-reexported_symbols_list,$t/list -o $t/libbad.dylib 2> $t/log
grep -v '^+' $t/log | grep -A1 _missing | grep -qF 'the command line'

# Only a dylib re-exports symbols: ld-prime refuses the list, empty or
# not, for any other output, though an executable may re-export a
# library whole (LC_REEXPORT_DYLIB).
: > $t/empty
msg='-reexported_symbols_list can only used used when created dynamic libraries'
not $CC --ld-path=$mold $t/main.o $t/libouter.dylib -o $t/exe2 \
  -Wl,-reexported_symbols_list,$t/empty 2> $t/log2
grep -qF -- "$msg" $t/log2
echo _foo > $t/list
not $CC --ld-path=$mold -bundle $t/a.o $t/libinner.dylib -o $t/b.bundle \
  -Wl,-reexported_symbols_list,$t/list 2> $t/log3
grep -qF -- "$msg" $t/log3
not $mold -arch $ARCH -r $t/a.o -o $t/r.o -reexported_symbols_list $t/list 2> $t/log4
grep -qF -- "$msg" $t/log4
$CC --ld-path=$mold $t/main.o -Wl,-reexport_library,$t/libouter.dylib -o $t/exe3
otool -l $t/exe3 | grep -q LC_REEXPORT_DYLIB

# A symbol of a library the image re-exports whole is exported already:
# ld-prime warns of each such name an export list or the re-export
# list gives, naming the file that defines it - a private library the
# re-exported one re-exports in turn - and adds no entry.
echo 'int baz() { return 3; }' |
  $CC --ld-path=$mold -dynamiclib -xc - -o $t/libbaz.dylib -install_name $PWD/$t/libbaz.dylib
echo 'int foo() { return 42; }' |
  $CC --ld-path=$mold -dynamiclib -xc - -o $t/libbar.dylib -install_name $PWD/$t/libbar.dylib \
    -Wl,-reexport_library,$t/libbaz.dylib
printf '_foo\n_baz\n' > $t/list5
$CC --ld-path=$mold -dynamiclib $t/a.o -o $t/libredundant.dylib \
  -Wl,-reexport_library,$t/libbar.dylib -Wl,-reexported_symbols_list,$t/list5 2> $t/log5
grep -qF "warning: explicit re-export for symbol '_baz' is redundant because it is already re-exported from dylib '$t/libbar.dylib'" $t/log5
grep -qF "warning: explicit re-export for symbol '_foo' is redundant because it is already re-exported from dylib '$t/libbar.dylib'" $t/log5
dyld_info -exports $t/libredundant.dylib > $t/exports5
not grep -q re-export $t/exports5
