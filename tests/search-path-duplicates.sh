#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -L or -F directory given again, spelled the same, is taken (and
# warned of) the first time only; spelled otherwise, or met again among
# the default directories, it is searched once more.
root=$t/root
mkdir -p $t/a $t/b $root/usr/lib

echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

$mold -arch $ARCH -platform_version macos 13.0 13.0 -o $t/exe $t/a.o \
  $SDK/usr/lib/libSystem.tbd -syslibroot $root -v -L$t/a -L$t/b -L $t/a \
  -L./$t/a -L/usr/lib -L/usr/lib -L/usr/lib/ -F$t/b -F$t/a -F$t/b \
  -L$t/none -L$t/none -F$t/none 2> $t/log > /dev/null

tab=$(printf '\t')
grep -E "^(Library search paths:|Framework search paths:|$tab)" $t/log > $t/paths
cat > $t/expected <<EOF
Library search paths:
$tab$t/a
$tab$t/b
$tab./$t/a
$tab$root/usr/lib
$tab$root/usr/lib/
$tab$root/usr/lib
Framework search paths:
$tab$t/b
$tab$t/a
EOF
diff $t/expected $t/paths
[ "$(grep -c "search path '$t/none' not found" $t/log)" -eq 2 ]
