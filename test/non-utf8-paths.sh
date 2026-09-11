#!/usr/bin/env bash
. $(dirname $0)/common.inc

input=$'input\xff object.o'
dir=$'dir\xfe'
response=$'args\xff.rsp'
output=$'result\xfe.o'

echo 'int foo() { return 42; }' | $CC -c -o "$t/$input" -xc -
./mold -r -o $t/argv.o "$t/$input"

printf '"%s"\n' "$t/$input" > "$t/$response"
printf '"@%s"\n' "$t/$response" > $t/nested.rsp
./mold -r -o $t/response.o @$t/nested.rsp

mkdir -p "$t/$dir"
cp "$t/$input" "$t/$dir/libraw.a.o"
ar crs "$t/$dir/libraw.a" "$t/$dir/libraw.a.o"
echo '' | $CC -c -o $t/empty.o -xc -
./mold -r $t/empty.o -o $t/library.o -L"$t/$dir" --whole-archive -lraw

printf 'INPUT("%s")\n' "$input" > "$t/$dir.script"
./mold -r -o $t/script.o -T "$t/$dir.script"

ar crsT $t/thin.a "$t/$input"
./mold -r -o $t/thin.o --whole-archive $t/thin.a

printf '"--output=%s" "%s"\n' "$t/$output" "$t/$input" > $t/output.rsp
./mold -r --repro @$t/output.rsp

for file in argv.o response.o library.o script.o thin.o "$output"; do
  readelf -Ws "$t/$file" | grep -E ' FUNC +GLOBAL +DEFAULT .* [0-9]+ +foo$'
done

# Replaying a reproduction must retain the raw names and spaces too.
ld=$PWD/mold
rm -rf "$t/$output.repro"
tar -C $t -xf "$t/$output.repro.tar"
(cd "$t/$output.repro"; "$ld" @response.txt)
readelf -Ws "$t/$output.repro$PWD/$t/$output" | grep -E ' FUNC +GLOBAL +DEFAULT .* [0-9]+ +foo$'
