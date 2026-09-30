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
try -Wl,-sectcreate,__X,__y,$t/nosuch.bin
grep -q "file cannot be open()ed, errno=2 (No such file or directory) path=$t/nosuch.bin" $t/log
