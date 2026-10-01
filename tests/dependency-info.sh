#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The file is opcode-prefixed NUL-terminated strings: 0x00 the linker's
# version, 0x10 an input, 0x11 a file looked for and missing, 0x40 an
# output, sorted by opcode and then path. Prints them a line each.
entries() {
  python3 - "$1" <<'EOF'
import sys
data = open(sys.argv[1], 'rb').read()
while data:
    end = data.index(b'\0', 1)
    print('%02x %s' % (data[0], data[1:end].decode('utf-8', 'replace').replace('\n', '\\n')))
    data = data[end + 1:]
EOF
}

mkdir -p $t/dir
rm -f $t/link
ln -s dir $t/link

cat <<EOF | $CC -o $t/dir/a.o -c -xc -
int main() {}
EOF
echo 'int unused() { return 0; }' | $CC -o $t/dir/b.o -c -xc -
rm -f $t/dir/libb.a
ar rcs $t/dir/libb.a $t/dir/b.o
echo $t/link/a.o > $t/dir/filelist
echo hello > $t/dir/sect.txt
echo _main > $t/dir/exports

rm -f $t/exe $t/map
$CC --ld-path=$mold -o $t/exe -Wl,-filelist,$t/link/filelist $t/link/libb.a \
  -Wl,-sectcreate,__TEXT,__hello,$t/link/sect.txt -Wl,-exported_symbols_list,$t/link/exports \
  -Wl,-dependency_info,$t/deps -Wl,-map,$t/map
$t/exe
entries $t/deps > $t/deps.txt

# The version is the linker's -v banner, and the entries are sorted.
head -1 $t/deps.txt | grep -q '^00 .*\\n$'
[ "$(tail -n +2 $t/deps.txt)" = "$(tail -n +2 $t/deps.txt | LC_ALL=C sort)" ]

# A relative path is resolved to the file's real path: every file the
# command line names, an archive whether or not a member loads, and the
# -filelist and -sectcreate files (but not a symbol list).
dir=$(cd $t/dir && pwd -P)
grep -qx "10 $dir/a.o" $t/deps.txt
grep -qx "10 $dir/libb.a" $t/deps.txt
grep -qx "10 $dir/filelist" $t/deps.txt
grep -qx "10 $dir/sect.txt" $t/deps.txt
not grep -q exports $t/deps.txt

# The libraries loaded as another's re-exports are listed twice each.
grep -q '^10 .*/usr/lib/libSystem.tbd$' $t/deps.txt
[ "$(grep -c '^10 .*/usr/lib/system/libsystem_c.tbd$' $t/deps.txt)" = 2 ]

# The outputs are the image and the map, as named before the link wrote
# them.
grep -qx "40 $t/exe" $t/deps.txt
grep -qx "40 $t/map" $t/deps.txt

# A -r link writes it too; Xcode asks its prelinks for one and fails the
# build if the file is missing. Its output resolves too once it exists.
rm -f $t/r.o
$mold -r -arch $ARCH -o $t/r.o $t/link/a.o -dependency_info $t/deps-r
entries $t/deps-r > $t/deps-r.txt
grep -qx "10 $dir/a.o" $t/deps-r.txt
grep -qx "40 $t/r.o" $t/deps-r.txt
$mold -r -arch $ARCH -o $t/r.o $t/link/a.o -dependency_info $t/deps-r
entries $t/deps-r > $t/deps-r.txt
grep -qx "40 $(cd $t && pwd -P)/r.o" $t/deps-r.txt

# A relative path that is a symbolic link resolves to the file it links
# to, and one spelled in another case than the file's, where the file
# system ignores case, to the file's name in its own case.
rm -f $t/dir/c.o
ln -s b.o $t/dir/c.o
$mold -r -arch $ARCH -o $t/r.o $t/link/a.o $t/link/c.o -dependency_info $t/deps-r
entries $t/deps-r > $t/deps-r.txt
grep -qx "10 $dir/a.o" $t/deps-r.txt
grep -qx "10 $dir/b.o" $t/deps-r.txt
if [ -e $t/LINK/A.O ]; then
  $mold -r -arch $ARCH -o $t/r.o $t/LINK/A.O $t/link/B.O -dependency_info $t/deps-r
  entries $t/deps-r > $t/deps-r.txt
  grep -qx "10 $dir/a.o" $t/deps-r.txt
  grep -qx "10 $dir/b.o" $t/deps-r.txt
fi

# The files the searches for inputs looked for and didn't find are
# listed as missing, each once: for -lfoo, the stub, the dylib, the .so
# and the archive in each directory until one is there, and the dylib
# next to a stub found; for a re-exported library, its leaf in each
# library directory, then its install name under the SDK; for a
# framework, its stub and its binary in each framework directory.
mkdir -p $t/L1 $t/L2 $t/F1 $t/F2/Bar.framework
echo 'int foo() { return 0; }' | $CC -o $t/foo.o -c -xc -
$CC -o $t/L2/libfoo.dylib -dynamiclib $t/foo.o -install_name @rpath/libfoo.dylib
$CC -o $t/F2/Bar.framework/Bar -dynamiclib $t/foo.o -install_name @rpath/Bar.framework/Bar
sdk=$(xcrun --show-sdk-path)
$mold -arch $ARCH -o $t/exe2 $t/dir/a.o -L$t/L1 -L$t/L2 -lfoo -F$t/F1 -F$t/F2 \
  -framework Bar -lSystem -syslibroot $sdk -dependency_info $t/deps2
entries $t/deps2 > $t/deps2.txt
for f in libfoo.tbd libfoo.dylib libfoo.so libfoo.a libSystem.tbd libSystem.a; do
  grep -qx "11 $t/L1/$f" $t/deps2.txt
done
grep -qx "11 $t/L2/libfoo.tbd" $t/deps2.txt
grep -qx "10 $(cd $t && pwd -P)/L2/libfoo.dylib" $t/deps2.txt
not grep -q "$t/L2/libfoo.so" $t/deps2.txt
grep -qx "11 $sdk/usr/lib/libSystem.dylib" $t/deps2.txt
grep -qx "11 $t/L1/libsystem_c.tbd" $t/deps2.txt
grep -qx "11 $t/L2/libsystem_c.dylib" $t/deps2.txt
grep -qx "11 $sdk/usr/lib/system/libsystem_c.dylib" $t/deps2.txt
grep -qx "11 $t/F1/Bar.framework/Bar.tbd" $t/deps2.txt
grep -qx "11 $t/F1/Bar.framework/Bar" $t/deps2.txt
grep -qx "11 $t/F2/Bar.framework/Bar.tbd" $t/deps2.txt
[ "$(grep -c "^11 $t/L1/libsystem_c.tbd$" $t/deps2.txt)" = 1 ]
