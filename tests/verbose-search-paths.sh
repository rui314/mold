#!/bin/bash
source "$(dirname "$0")"/common.inc

# -v prints the banner once the options are checked (after their
# warnings), then the library and the framework search paths on
# stderr, a tab before each: the -L and -F directories, then the
# default ones, as they are looked up under the syslibroot. -v with
# nothing to link prints the banner alone.
sdk=$(xcrun --show-sdk-path)
root=$t/root
mkdir -p $t/lib $t/fw $root/opt/x $root/usr/lib/swift $root/System/Library/Frameworks

echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

link() {
  $mold -arch $ARCH -platform_version macos 13.0 13.0 -o $t/exe $t/a.o \
    $sdk/usr/lib/libSystem.tbd -syslibroot $root -v "$@"
}

tab=$(printf '\t')
paths() {
  grep -E "^(Library search paths:|Framework search paths:|$tab)" "$@"
}

link -L$t/lib -L/opt/x -F$t/fw -headerpad 0x10 -v > $t/log 2> $t/err
paths $t/err > $t/paths
cat > $t/expected <<EOF
Library search paths:
$tab$t/lib
$tab$root/opt/x
$tab$root/usr/lib
$tab$root/usr/lib/swift
Framework search paths:
$tab$t/fw
$tab$root/System/Library/Frameworks
EOF
diff $t/expected $t/paths

# The banner comes once, after the options' warnings and before the
# paths.
link -L$t/lib -headerpad 0x10 > $t/log2 2>&1
banner='^@(#)PROGRAM:ld\|^mold-macho'
[ "$(grep -c "$banner" $t/log2)" = 1 ]
grep -n "warning: -headerpad\\|$banner\\|^Library search paths" $t/log2 | cut -d: -f1 > $t/lines
[ "$(sort -n $t/lines)" = "$(cat $t/lines)" ]
[ "$(wc -l < $t/lines)" -eq 3 ]

link -Z -L$t/lib -F$t/fw 2> $t/err3 > /dev/null
paths $t/err3 > $t/paths3
cat > $t/expected3 <<EOF
Library search paths:
$tab$t/lib
Framework search paths:
$tab$t/fw
EOF
diff $t/expected3 $t/paths3

$mold -v -arch $ARCH -L$t/lib > $t/log4 2>&1
not grep -q 'search paths' $t/log4
