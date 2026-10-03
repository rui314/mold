#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime creates the output afresh, removing an existing file first,
# with permissions 0777 for an image and 0644 for an object, less the
# umask. An existing file it may not write is an error. A character
# device (/dev/null) is written in place, as is a file in a directory
# it may not write, which keeps its mode; a FIFO is replaced. The
# file must take the size of the output (ftruncate), which a pipe
# can't.
cat <<EOF | $CC -o $t/a.o -c -xc -
int main(void) { return 0; }
EOF
link() { $mold -arch $ARCH -platform_version macos 13.0 13.0 -syslibroot "$(xcrun --show-sdk-path)" -lSystem $t/a.o "$@"; }

link -o /dev/null
$mold -r -arch $ARCH $t/a.o -o /dev/null

rm -f $t/exe $t/r.o $t/lib.dylib
(umask 022; link -o $t/exe; link -dylib -o $t/lib.dylib; $mold -r -arch $ARCH $t/a.o -o $t/r.o)
[ "$(stat -f %Lp $t/exe $t/lib.dylib $t/r.o | tr '\n' ' ')" = '755 755 644 ' ]
(umask 027; link -o $t/exe; $mold -r -arch $ARCH $t/a.o -o $t/r.o)
[ "$(stat -f %Lp $t/exe $t/r.o | tr '\n' ' ')" = '750 640 ' ]
(umask 0; link -o $t/exe; $mold -r -arch $ARCH $t/a.o -o $t/r.o)
[ "$(stat -f %Lp $t/exe $t/r.o | tr '\n' ' ')" = '777 644 ' ]

# A hard link to the old file keeps the old contents.
echo old > $t/old
ln -f $t/old $t/exe
link -o $t/exe
[ "$(cat $t/old)" = old ]

rm -f $t/ro
touch $t/ro
chmod 444 $t/ro
not link -o $t/ro 2> $t/log
grep -q "can't write output file: $t/ro" $t/log
[ ! -s $t/ro ]

rm -rf $t/dir
mkdir $t/dir
echo hello > $t/dir/out
chmod 600 $t/dir/out
chmod 555 $t/dir
link -o $t/dir/out
chmod 755 $t/dir
[ "$(stat -f %Lp $t/dir/out)" = 600 ]
otool -h $t/dir/out | grep -q 0x

rm -f $t/fifo
mkfifo $t/fifo
link -o $t/fifo
[ -f $t/fifo ]

rm -f $t/dangling
ln -s $t/nonexistent $t/dangling
not link -o $t/dangling 2> $t/log
grep -q 'File exists' $t/log

not link -o $t/dir 2> $t/log
grep -q 'Is a directory' $t/log

link -o /dev/stdout 2> $t/log | cat > /dev/null || true
grep -q 'Invalid argument' $t/log
