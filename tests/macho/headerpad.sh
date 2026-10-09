#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image leaves free space after its load commands so that tools
# can add or grow commands in place: -headerpad, at least 32 bytes in an
# image dyld loads (room for codesign's LC_CODE_SIGNATURE, with a
# warning if -headerpad asks for less), or with
# -headerpad_max_install_names room for each dylib command to grow to
# MAXPATHLEN (1024). A -r output, which no tool adds commands to, gets
# none: its contents start right after its load commands, aligned for
# its sections. (ld-prime gives it -headerpad, 32 by default.)
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

# The bytes between the end of the load commands and the first section.
free() {
  local end=$((32 + $(otool -h $1 | tail -1 | awk '{print $7}')))
  local first=$(otool -l $1 | awk '$1 == "offset" && $2 > 0 { print $2; exit }')
  echo $((first - end))
}

$CC --ld-path=$mold -o $t/exe $t/a.o
[ $(free $t/exe) -ge 32 ]
$CC --ld-path=$mold -o $t/exe2 $t/a.o -mmacosx-version-min=11.0
[ $(free $t/exe2) -ge 32 ]
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-headerpad,0x100
[ $(free $t/exe3) -ge 256 ]
$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-headerpad,0x10 2> $t/log4
[ $(free $t/exe4) -ge 32 ]
grep -q -- '-headerpad 0x10 is too small, at least 32 bytes are required to reserve space for code signature' $t/log4
$CC --ld-path=$mold -o $t/exe5 $t/a.o -lz -Wl,-headerpad_max_install_names
[ $(free $t/exe5) -ge 2048 ]
$RUN $t/exe5
$CC --ld-path=$mold -o $t/exe6 $t/a.o -Wl,-headerpad,0x1000
[ $(free $t/exe6) -ge 4096 ]
$RUN $t/exe6

# The free space is what install_name_tool needs to rename a dylib in
# place, and codesign to add its command.
cat <<EOF | $CC -o $t/b.o -c -xc -
int foo() { return 3; }
EOF
cat <<EOF | $CC -o $t/c.o -c -xc -
int foo();
int main() { return foo() != 3; }
EOF
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/b.o -Wl,-install_name,@rpath/libfoo.dylib
$CC --ld-path=$mold -o $t/exe7 $t/c.o -L$t -lfoo -Wl,-headerpad_max_install_names
long=$t/$(printf 'x%.0s' $(seq 1 200))
mkdir -p $long
cp $t/libfoo.dylib $long/libfoo.dylib
install_name_tool -change @rpath/libfoo.dylib $long/libfoo.dylib $t/exe7 2> /dev/null
codesign -f -s - $t/exe7 2> /dev/null
otool -L $t/exe7 | grep -q "$long/libfoo.dylib"
$RUN $t/exe7

# Without -headerpad_max_install_names the room is no less than
# ld-prime's, which places the sections after an estimate of the load
# commands that counts up to 8 bytes more per dependency. Post-link
# scripts written against Xcode spend it: Sequel Ace's renames dylibs
# installed under bare names to @loader_path ones, 16 bytes per
# command, here 48 bytes in all, more than -headerpad's 32.
for i in 1 2 3; do
  echo "int bar$i() { return $i; }" | $CC -o $t/d$i.o -c -xc -
  $CC --ld-path=$mold -shared -o $t/libbar$i.dylib $t/d$i.o -Wl,-install_name,libbar$i.dylib
done
cat <<EOF | $CC -o $t/e.o -c -xc -
int bar1(), bar2(), bar3();
int main() { return bar1() + bar2() + bar3() != 6; }
EOF
$CC --ld-path=$mold -o $t/exe8 $t/e.o -L$t -lbar1 -lbar2 -lbar3
for i in 1 2 3; do
  install_name_tool -change libbar$i.dylib @loader_path/libbar$i.dylib $t/exe8 2> /dev/null
done
codesign -f -s - $t/exe8 2> /dev/null
[ $(otool -L $t/exe8 | grep -c '@loader_path/libbar') = 3 ]
$RUN $t/exe8

# An image no dyld loads gets -headerpad, whatever it says.
$mold -arch $ARCH -static -e _main -o $t/static $t/a.o
[ $(free $t/static) -ge 32 ]
$mold -arch $ARCH -static -e _main -headerpad 0 -o $t/static2 $t/a.o 2> $t/log7
not grep -q 'too small' $t/log7

# Checks that the first section of a -r output starts right after the
# load commands, rounded up to the largest section alignment.
r_gap() {
  local end=$((32 + $(otool -h $1 | tail -1 | awk '{print $7}')))
  local first=$(otool -l $1 | awk '$1 == "offset" && $2 > 0 { print $2; exit }')
  local align=$((1 << $(otool -l $1 | awk '$1 == "align" { sub(/2\^/, "", $2); if ($2 + 0 > m) m = $2 + 0 }
    END { print m + 0 }')))
  [ $first = $(( (end + align - 1) / align * align )) ]
}

$mold -arch $ARCH -r -o $t/r.o $t/a.o
r_gap $t/r.o
$mold -arch $ARCH -r -headerpad 0x10 -o $t/r2.o $t/a.o
r_gap $t/r2.o
