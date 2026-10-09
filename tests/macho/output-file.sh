#!/bin/bash
source "$(dirname "$0")"/common.inc

# The output is created afresh, an existing file removed first, with
# permissions 0777 for an image and 0644 for an object, less the umask.
# A character device (/dev/null) is written in place, as is a file in a
# directory the link may not write, which keeps its mode. The file must
# take the size of the output, which a pipe can't.
cat <<EOF | $CC -o $t/a.o -c -xc -
int main(void) { return 0; }
EOF
link() { $mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -syslibroot "$SDK" -lSystem $t/a.o "$@"; }

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

not link -o $t/dir 2> $t/log
grep -q 'Is a directory' $t/log

link -o /dev/stdout 2> $t/log | cat > /dev/null || true
grep -q 'Invalid argument' $t/log

# A read-only file, or a symbolic link, is replaced. (ld-prime refuses
# the one, and fails to create a file where the other dangles.)
if $mold -v 2>&1 | grep -q mold-macho; then
  rm -f $t/ro
  touch $t/ro
  chmod 444 $t/ro
  link -o $t/ro
  otool -h $t/ro | grep -q 0x

  rm -f $t/dangling
  ln -s $t/nonexistent $t/dangling
  link -o $t/dangling
  [ -f $t/dangling ] && [ ! -L $t/dangling ] && [ ! -e $t/nonexistent ]
fi
