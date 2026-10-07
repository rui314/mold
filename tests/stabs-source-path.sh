#!/bin/bash
source "$(dirname "$0")"/common.inc

# The two N_SO notes that open an object's run split the source file's
# path at its last slash: the compile unit's name, under its
# compilation directory unless absolute, joined as they are.
mkdir -p $t/sub
echo 'int f1(void) { return 1; }' > $t/sub/f1.c
echo 'int f2(void) { return 2; }' > $t/sub/f2.c
echo 'int f3(void) { return 3; }' > $t/sub/f3.c
echo 'int f4(void) { return 4; }' > $t/f4.c
dir=$(cd $t && pwd)
(cd $t && $CC -g -c sub/f1.c -o f1.o)
$CC -g -c $dir/sub/f2.c -o $t/f2.o
(cd $t && $CC -g -fdebug-compilation-dir=. -c sub/f3.c -o f3.o)
(cd $t && $CC -g -fdebug-compilation-dir=/ -c f4.c -o f4.o)

# N_OSO names an object of a fat file, or a member of a fat archive,
# by the file's own path, and has the file's modification time.
echo 'int af(void) { return 5; }' > $t/a.c
echo 'int of(void) { return 6; }' > $t/o.c
for arch in arm64 x86_64; do
  cc -arch $arch -g -c $t/a.c -o $t/a-$arch.o
  rm -f $t/liba-$arch.a
  ar rcs $t/liba-$arch.a $t/a-$arch.o
  cc -arch $arch -g -c $t/o.c -o $t/o-$arch.o
done
lipo -create $t/liba-arm64.a $t/liba-x86_64.a -output $t/libfat.a
lipo -create $t/o-arm64.o $t/o-x86_64.o -output $t/fat.o

cat <<EOF | $CC -o $t/main.o -c -xc -
int f1(void), f2(void), f3(void), f4(void), af(void), of(void);
int main(void) { return f1() + f2() + f3() + f4() + af() + of() - 21; }
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/f1.o $t/f2.o $t/f3.o $t/f4.o $t/libfat.a $t/fat.o
$RUN $t/exe

nm -ap $t/exe | awk '$5 == "SO" || $5 == "OSO" { print $1, $5, $6 }' > $t/stabs
grep -A1 " SO $dir/sub/\$" $t/stabs | grep -q ' SO f1.c$'
[ "$(grep -A1 " SO $dir/sub/\$" $t/stabs | grep -c ' SO f[12].c$')" = 2 ]
grep -A1 ' SO \./sub/$' $t/stabs | grep -q ' SO f3.c$'
grep -A1 ' SO //$' $t/stabs | grep -q ' SO f4.c$'
grep -q " OSO $dir/libfat.a(a-$ARCH.o)\$" $t/stabs
mtime=$(printf '%016x' $(stat -f %m $t/fat.o))
grep -q "^$mtime OSO $dir/fat.o\$" $t/stabs
