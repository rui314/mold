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
