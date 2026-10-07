#!/bin/bash
source "$(dirname "$0")"/common.inc

# Paths, install names and rpaths are bytes, not text: a cross link's
# host may name files outside UTF-8. Every byte has to reach the file
# system, the load commands, the stabs and the reports unchanged.
#
# APFS itself refuses file names that are not UTF-8, so the paths here
# use non-ASCII UTF-8 and spaces; the load-command strings, which never
# touch the file system, carry arbitrary bytes.
input=$'input é object.o'
dir=$'dir ü'
lib=$'raw ö'
response=$'args é.rsp'
output=$'result ü'
install=$'@rpath/lib\xfe.dylib'
rpath=$'/opt/lib\xff'
link="$mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -syslibroot $SDK -lSystem"

echo 'int foo() { return 42; }' | $CC -g -c -o "$t/$input" -xc -
echo 'int foo(); int main() { return foo() - 42; }' | $CC -c -o $t/main.o -xc -

# A direct argument, the output path, -install_name and -rpath in the
# load commands, the object's path in the N_OSO stab and in the -map
# and -dependency_info reports.
$link -dylib "$t/$input" -o "$t/$output.dylib" -install_name "$install" \
  -rpath "$rpath" -map "$t/$output.map" -dependency_info "$t/$output.dep"
grep -aF "$install" "$t/$output.dylib"
grep -aF "$rpath" "$t/$output.dylib"
nm -ap "$t/$output.dylib" | grep -aF "$input"
grep -aF "$t/$input" "$t/$output.map"
grep -aF "$t/$input" "$t/$output.dep"
grep -aF "$t/$output.dylib" "$t/$output.dep"

# Quoted response file tokens, one response file nested in another, and
# a byte string that is not UTF-8 passing through the tokenizer.
printf '"%s" -install_name "%s"\n' "$t/$input" "$install" > "$t/$response"
printf '"@%s" -o "%s"\n' "$t/$response" "$t/response.dylib" > $t/nested.rsp
$link -dylib @$t/nested.rsp
nm "$t/response.dylib" | grep ' T _foo'
grep -aF "$install" "$t/response.dylib"

# -filelist: the list's own name, its entries, and the directory prefix.
printf '%s\n' "$t/$input" > "$t/$dir.list"
$mold -r -o $t/filelist.o -filelist "$t/$dir.list"
nm $t/filelist.o | grep ' T _foo'
printf '%s\n' "$input" > $t/relative.list
$mold -r -o $t/filelist2.o -filelist "$t/relative.list,$t"
nm $t/filelist2.o | grep ' T _foo'

# Library search: a -L directory and a -l name with such bytes, whose
# archive holds a member named that way, force-loaded so its N_OSO
# names the archive and the member; -sectcreate reads such a path.
mkdir -p "$t/$dir"
ar crs "$t/$dir/lib$lib.a" "$t/$input"
$link -o $t/exe $t/main.o -L"$t/$dir" -l"$lib" -all_load \
  -sectcreate __DATA __blob "$t/$input"
$RUN $t/exe
nm -ap $t/exe | grep -aF "lib$lib.a($input)"
otool -l $t/exe | grep 'sectname __blob'
