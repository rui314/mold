#!/bin/bash
source "$(dirname "$0")"/common.inc

# How ld-prime words an input it can't find or open: a library or
# framework option by the name it was given, whatever the option's
# flavor (-weak-l, -needed-l, -reexport-l...); a library named by path
# (-force_load, -weak_library, -bundle_loader...) as a library; an
# object by its path and why; a list file by its option too.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

try() { not $CC --ld-path=$mold -o $t/exe $t/a.o "$@" 2> $t/log; }
# says <word>... <reason>: the log names each word and the reason.
says() {
  grep -v '^+' $t/log > $t/msgs || true
  for word in "$@"; do grep -qF -- "$word" $t/msgs; done
}

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
says $t/nosuch.o 'No such file or directory'
mkdir -p $t/dir
try -Wl,$t/dir
says $t/dir 'Invalid argument'
: > $t/empty.o
try -Wl,$t/empty.o
grep -qF "file is empty in '$t/empty.o'" $t/log
try -Wl,-force_load,$t/nosuch.a
grep -q "library '$t/nosuch.a' not found" $t/log
try -Wl,-weak_library,$t/nosuch.dylib
grep -q "library '$t/nosuch.dylib' not found" $t/log
try -Wl,-exported_symbols_list,$t/nosuch.txt
says -exported_symbols_list $t/nosuch.txt 'No such file or directory'
try -Wl,-unexported_symbols_list,$t/nosuch.txt
says -unexported_symbols_list $t/nosuch.txt 'No such file or directory'
try -Wl,-filelist,$t/nosuch.txt
says -filelist $t/nosuch.txt 'No such file or directory'
try -Wl,-sectcreate,__X,__y,$t/nosuch.bin
says $t/nosuch.bin 'No such file or directory'

# A missing order file or alias list only costs the order or the
# aliases: a warning says so, and the link goes on.
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-order_file,$t/nosuch.txt 2> $t/log
says $t/nosuch.txt 'No such file or directory'
$t/exe2
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-alias_list,$t/nosuch.txt 2> $t/log
says $t/nosuch.txt 'No such file or directory'
$t/exe3

# The first library or framework not found, in command-line order
# (-force_load's among them), stops the link: one is reported. (ld-prime
# looks the frameworks up only after the libraries.)
try -framework NoSuch1 -Wl,-force_load,$t/nosuch.a -lnosuch -lnosuch -framework NoSuch2
grep -v '^+' $t/log > $t/msgs
[ "$(grep -c 'not found' $t/msgs)" = 1 ]
grep -q "framework 'NoSuch1' not found" $t/msgs
try -Wl,-force_load,$t/nosuch.a -lnosuch -lnosuch -framework NoSuch2
grep -v '^+' $t/log > $t/msgs
[ "$(grep -c 'not found' $t/msgs)" = 1 ]
grep -q "library '$t/nosuch.a' not found" $t/msgs
try -framework NoSuch1 -framework NoSuch2
grep -v '^+' $t/log > $t/msgs
[ "$(grep -c 'not found' $t/msgs)" = 1 ]
grep -q "framework 'NoSuch1' not found" $t/msgs

# A bare path ending in .a is a library's, though one in a -filelist is
# a file's like any other.
try -Wl,$t/nosuch2.o -Wl,$t/nosuch.a -framework NoSuch
grep -q "library '$t/nosuch.a' not found" $t/log
try -Wl,$t/nosuch.a -lnosuch
grep -q "library '$t/nosuch.a' not found" $t/log
echo $t/nosuch.a > $t/list
try -framework NoSuch -Wl,-filelist,$t/list
grep -q "framework 'NoSuch' not found" $t/log

# A library search finds what is there, a directory too, which ld-prime
# then fails to map.
mkdir -p $t/libdir/libdir.dylib $t/fwdir/Dir.framework/Dir
try -L$t/libdir -ldir
says $t/libdir/libdir.dylib 'Invalid argument'
try -F$t/fwdir -framework Dir
says $t/fwdir/Dir.framework/Dir 'Invalid argument'

# The errors in several input files are all reported.
try -Wl,$t/nosuch.o -Wl,$t/empty.o
says $t/nosuch.o 'No such file or directory' "file is empty in '$t/empty.o'"

# A file it can't link it words by what the file is.
echo 'int x;' > $t/x.c
try -Wl,$t/x.c
grep -qF "unknown file type in '$t/x.c'" $t/log
try -Wl,$t/exe2
grep -qF "unsupported mach-o filetype (only MH_OBJECT and MH_DYLIB can be linked) in '$t/exe2'" $t/log
