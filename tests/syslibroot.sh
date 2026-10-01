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
grep -q 'file cannot be open()ed' $t/log

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
