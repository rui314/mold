#!/bin/bash
source "$(dirname "$0")"/common.inc

# A search directory is looked up under every -syslibroot in turn and
# searched in each root that has it; one that no root has is searched
# where it is, and so is a default directory, unless there is just one
# root. A -syslibroot / anywhere puts no directory under a root, though
# the roots still hold the files -weak_library and the like name. An
# absolute directory that climbs with /.. is resolved first, and one
# that starts with // replaces the root.
abs=$(cd $t && pwd -P)
r1=$t/r1
r2=$t/r2
mkdir -p $abs/a/b $abs/c $r1$abs/a $r2$abs/a $r1/usr/lib $r2/usr/local/lib \
  $r1/System/Library/Frameworks $r1/opt/lib
ln -sfn a/b $abs/sym

echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

tab=$(printf '\t')
paths() {
  $mold -arch $ARCH -platform_version macos 13.0 13.0 -o $t/exe $t/a.o \
    $SDK/usr/lib/libSystem.tbd -v "$@" 2> $t/log > /dev/null
  grep -E "^(Library search paths:|Framework search paths:|$tab)" $t/log
}
host() {
  if [ -d $1 ]; then echo "$tab$1"; fi
}

paths -syslibroot $r1 -syslibroot $r2 -L$abs/a -L$abs/c > $t/paths
cat > $t/expected <<EOF
Library search paths:
$tab$r1$abs/a
$tab$r2$abs/a
$tab$abs/c
$tab$r1/usr/lib
$(host /usr/lib/swift)
$tab$r2/usr/local/lib
Framework search paths:
$(host /Library/Frameworks)
$tab$r1/System/Library/Frameworks
EOF
grep . $t/expected | diff - $t/paths

paths -Z -syslibroot $r1 -syslibroot / -L$abs/a > $t/paths2
printf 'Library search paths:\n\t%s\nFramework search paths:\n' $abs/a | diff - $t/paths2
paths -Z -syslibroot / -syslibroot $r1 -L$abs/a > $t/paths3
diff $t/paths2 $t/paths3

paths -Z -syslibroot $r1 -L$abs/sym/.. -L/$abs/c > $t/paths4
printf 'Library search paths:\n\t%s\n\t%s\nFramework search paths:\n' \
  $r1$abs/a $abs/c | diff - $t/paths4
paths -Z -L$abs/sym/.. > $t/paths5
printf 'Library search paths:\n\t%s\nFramework search paths:\n' $abs/a | diff - $t/paths5

cat > $r1/opt/lib/libqux.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '/opt/lib/libqux.dylib'
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _qux ]
...
EOF
$mold -arch $ARCH -platform_version macos 13.0 13.0 -o $t/exe2 $t/a.o \
  $SDK/usr/lib/libSystem.tbd -syslibroot / -syslibroot $r1 \
  -weak_library /opt/lib/libqux.dylib 2> /dev/null
otool -L $t/exe2 | grep -q /opt/lib/libqux.dylib
