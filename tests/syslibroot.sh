#!/bin/bash
source "$(dirname "$0")"/common.inc

mkdir -p $t/foo/bar

cat <<EOF | $CC -shared -o $t/foo/bar/libbaz.dylib -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo();
void bar() { foo(); }
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o -nodefaultlibs \
  -L/foo/bar -isysroot $t -lbaz

# An option that takes a library's path looks an absolute one up under
# the syslibroot first, a stub in place of the dylib; a bare path is a
# file as it is.
mkdir -p $t/root/opt/lib
cat > $t/root/opt/lib/libqux.tbd <<EOT
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '/opt/lib/libqux.dylib'
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _qux ]
...
EOT
echo 'void qux(void); void f(void) { qux(); }' | $CC -o $t/q.o -c -xc -
for opt in -weak_library -needed_library -reexport_library -upward_library -lazy_library; do
  $CC --ld-path=$mold -shared -o $t/q.dylib $t/q.o -Wl,-syslibroot,$t/root \
    -Wl,$opt,/opt/lib/libqux.dylib -Wl,-undefined,dynamic_lookup 2> /dev/null
  otool -L $t/q.dylib | grep -q /opt/lib/libqux.dylib
done
not $CC --ld-path=$mold -shared -o $t/q.dylib $t/q.o -Wl,-syslibroot,$t/root \
  -Wl,/opt/lib/libqux.dylib 2> $t/log
grep -q 'No such file or directory' $t/log

# But for an archive's: ld-prime looks a bare path ending in .a up as
# -force_load's path, under the syslibroot first, even when the path
# names a file of its own, and merges what the options naming the file
# say of it.
mkdir -p $t/root$PWD/$t/ar $t/ar
echo 'int qux_a(void) { return 3; }' | $CC -o $t/qa.o -c -xc -
echo 'int qux_b(void) { return 4; }' | $CC -o $t/qb.o -c -xc -
rm -f $t/root$PWD/$t/ar/libqa.a $t/ar/libqa.a
ar rcs $t/root$PWD/$t/ar/libqa.a $t/qa.o
ar rcs $t/ar/libqa.a $t/qb.o
echo 'int qux_a(void); int g(void) { return qux_a(); }' | $CC -o $t/g.o -c -xc -
$CC --ld-path=$mold -shared -o $t/g.dylib $t/g.o -Wl,-syslibroot,$t/root \
  -Wl,$PWD/$t/ar/libqa.a
nm -m $t/g.dylib | grep -q 'external _qux_a'
$CC --ld-path=$mold -shared -o $t/g.dylib $t/g.o -Wl,-syslibroot,$t/root \
  -Wl,$PWD/$t/ar/libqa.a -Wl,-load_hidden,$PWD/$t/ar/libqa.a
nm -m $t/g.dylib | grep -q 'non-external (was a private external) _qux_a'

# A last -syslibroot of / drops the roots, those before it too.
not $CC --ld-path=$mold -shared -o $t/q.dylib $t/q.o -Wl,-syslibroot,$t/root \
  -Wl,-syslibroot,/ -Wl,-weak_library,/opt/lib/libqux.dylib 2> $t/log
grep -q "library '/opt/lib/libqux.dylib' not found" $t/log

# Outside the syslibroot the path is the file itself, whether or not a
# stub sits next to it.
mkdir -p $t/lib1 $t/lib2
echo 'void qux(void) {}' | $CC -shared -o $t/lib1/libqux.dylib -xc - \
  -Wl,-install_name,@rpath/libqux1.dylib
cp $t/root/opt/lib/libqux.tbd $t/lib1/libqux.tbd
cp $t/root/opt/lib/libqux.tbd $t/lib2/libqux.tbd
for opt in -weak_library -needed_library -reexport_library -upward_library -lazy_library; do
  $CC --ld-path=$mold -shared -o $t/q.dylib $t/q.o -Wl,$opt,$t/lib1/libqux.dylib 2> /dev/null
  otool -L $t/q.dylib | grep -q @rpath/libqux1.dylib
  not $CC --ld-path=$mold -shared -o $t/q.dylib $t/q.o -Wl,$opt,$t/lib2/libqux.dylib 2> $t/log
  grep -q "library '$t/lib2/libqux.dylib' not found" $t/log
done

# A path of an archive (one ending in .a) on the command line is a
# library's to ld-prime: an absolute one is looked up under the
# syslibroot first, and one missing is a library not found. Any other
# path is the file as it is, as is an archive's a -filelist gives.
abs=$(cd $t && pwd)
mkdir -p $t/arc $t/root$abs/arc
echo 'int which(void) { return 1; }' | $CC -o $t/arc/one.o -c -xc -
echo 'int which(void) { return 2; }' | $CC -o $t/arc/two.o -c -xc -
rm -f $t/arc/libw.a $t/root$abs/arc/libw.a
ar rcs $t/arc/libw.a $t/arc/one.o
ar rcs $t/root$abs/arc/libw.a $t/arc/two.o
cat <<EOF2 | $CC -o $t/main.o -c -xc -
int which(void);
int main() { return which(); }
EOF2
$CC --ld-path=$mold -o $t/exe $t/main.o $abs/arc/libw.a -Wl,-syslibroot,$t/root
$t/exe || [ $? = 2 ]
echo $abs/arc/libw.a > $t/arc/list
$CC --ld-path=$mold -o $t/exe $t/main.o -Wl,-filelist,$t/arc/list -Wl,-syslibroot,$t/root
$t/exe || [ $? = 1 ]
not $CC --ld-path=$mold -o $t/exe $t/main.o -Wl,$t/arc/libnone.a 2> $t/log
grep -q "library '$t/arc/libnone.a' not found" $t/log
