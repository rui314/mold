#!/bin/bash
source "$(dirname "$0")"/common.inc

# How ld-prime words an input it can't find or open: a library or
# framework option by the name it was given, whatever the option's
# flavor (-weak-l, -needed-l, -reexport-l...); a library named by path
# (-force_load, -weak_library, -bundle_loader...) as a library; an
# object by errno and path; a list file by its option.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

try() { not $CC --ld-path=$mold -o $t/exe $t/a.o "$@" 2> $t/log; }

try -lnosuch
grep -q "library 'nosuch' not found" $t/log
try -Wl,-weak-lnosuch
grep -q "library 'nosuch' not found" $t/log
try -Wl,-needed-lnosuch
grep -q "library 'nosuch' not found" $t/log
try -framework NoSuch
grep -q "framework 'NoSuch' not found" $t/log
try -Wl,-weak_framework,NoSuch
grep -q "framework 'NoSuch' not found" $t/log
try -Wl,$t/nosuch.o
grep -q "file cannot be open()ed, errno=2 (No such file or directory) path=$t/nosuch.o in '$t/nosuch.o'" $t/log
mkdir -p $t/dir
try -Wl,$t/dir
grep -qF "file cannot be mmap()ed, errno=22 (Invalid argument) path=$t/dir in '$t/dir'" $t/log
: > $t/empty.o
try -Wl,$t/empty.o
grep -qF "file is empty in '$t/empty.o'" $t/log
try -Wl,-force_load,$t/nosuch.a
grep -q "library '$t/nosuch.a' not found" $t/log
try -Wl,-weak_library,$t/nosuch.dylib
grep -q "library '$t/nosuch.dylib' not found" $t/log
try -Wl,-exported_symbols_list,$t/nosuch.txt
grep -q "\-exported_symbols_list file '$t/nosuch.txt' could not be opened, errno=2 (No such file or directory)" $t/log
try -Wl,-unexported_symbols_list,$t/nosuch.txt
grep -q "\-unexported_symbols_list file '$t/nosuch.txt' could not be opened, errno=2" $t/log
try -Wl,-filelist,$t/nosuch.txt
grep -q "\-filelist file '$t/nosuch.txt' could not be opened, errno=2" $t/log
# ld-prime ends these errors with a blank line.
not $mold -o $t/exe -filelist $t/nosuch.txt 2> $t/log
grep -v '^+' $t/log | tail -1 > $t/last
not grep -q . $t/last
not $mold -o $t/exe $t/a.o -exported_symbols_list $t/nosuch.txt 2> $t/log
grep -v '^+' $t/log | tail -1 > $t/last
not grep -q . $t/last
try -Wl,-sectcreate,__X,__y,$t/nosuch.bin
grep -q "file cannot be open()ed, errno=2 (No such file or directory) path=$t/nosuch.bin" $t/log

# A missing order file or alias list only costs the order or the
# aliases: ld-prime warns, in its order-file words for both, and links.
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-order_file,$t/nosuch.txt 2> $t/log
grep -q "order file '$t/nosuch.txt' could not be opened, errno=2" $t/log
$t/exe2
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-alias_list,$t/nosuch.txt 2> $t/log
grep -q "order file '$t/nosuch.txt' could not be opened, errno=2" $t/log
$t/exe3

# ld-prime stops at the first library it doesn't find, looking the
# libraries up in command-line order (-force_load's among them), and the
# frameworks only after them: it reports one only.
try -framework NoSuch1 -Wl,-force_load,$t/nosuch.a -lnosuch -lnosuch -framework NoSuch2
grep -v '^+' $t/log > $t/msgs
[ "$(grep -c 'not found' $t/msgs)" = 1 ]
grep -q "library '$t/nosuch.a' not found" $t/msgs
try -framework NoSuch1 -framework NoSuch2
grep -v '^+' $t/log > $t/msgs
[ "$(grep -c 'not found' $t/msgs)" = 1 ]
grep -q "framework 'NoSuch1' not found" $t/msgs

# A library search finds what is there, a directory too, which ld-prime
# then fails to map.
mkdir -p $t/libdir/libdir.dylib $t/fwdir/Dir.framework/Dir
try -L$t/libdir -ldir
grep -qF "file cannot be mmap()ed, errno=22 (Invalid argument) path=$t/libdir/libdir.dylib" $t/log
try -F$t/fwdir -framework Dir
grep -qF "file cannot be mmap()ed, errno=22 (Invalid argument) path=$t/fwdir/Dir.framework/Dir" $t/log

# A file it can't link it words by what the file is.
echo 'int x;' > $t/x.c
try -Wl,$t/x.c
grep -qF "unknown file type in '$t/x.c'" $t/log
try -Wl,$t/exe2
grep -qF "unsupported mach-o filetype (only MH_OBJECT and MH_DYLIB can be linked) in '$t/exe2'" $t/log
