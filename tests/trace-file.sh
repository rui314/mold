#!/bin/bash
source "$(dirname "$0")"/common.inc

# -trace_file, -trace_file_shared_cache and -trace_symbols_file append a
# line of JSON each about what a final link linked: the output by name
# and UUID, the dylibs it loads (by real path, or by install name for
# the shared cache, which names the output by its path), weak ones also
# apart, and the archives a member
# loads from; -trace_symbols_file adds the symbols each dylib provides,
# the global symbols the loaded members of each archive define and the
# archives with a member that doesn't load. A file the link can't write
# fails it.
dir=$(cd $t && pwd -P)

echo 'int w1(void) { return 1; }' | $CC -o $t/w.o -c -xc -
$CC -o $t/libw.dylib -dynamiclib $t/w.o -install_name @rpath/libw.dylib
echo 'int l1(void) { return 1; }' | $CC -o $t/l1.o -c -xc -
echo 'int l2(void) { return 2; }' | $CC -o $t/l2.o -c -xc -
echo 'int z1(void) { return 3; }' | $CC -o $t/z.o -c -xc -
rm -f $t/libl.a $t/libz.a
ar rcs $t/libl.a $t/l1.o $t/l2.o
ar rcs $t/libz.a $t/z.o
cat <<EOF | $CC -o $t/a.o -c -xc -
int w1(void); int l1(void);
int main() { return w1() + l1(); }
EOF

rm -f $t/trace $t/trace-sc $t/trace-syms
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-weak_library,$t/libw.dylib $t/libl.a $t/libz.a \
  -Wl,-trace_file,$t/trace -Wl,-trace_file_shared_cache,$t/trace-sc \
  -Wl,-trace_symbols_file,$t/trace-syms
uuid=$(dwarfdump --uuid $t/exe | awk '{ print $2 }')

grep -Eqx "\{\"uuid\":\"$uuid\",\"name\":\"exe\",\"arch\":\"$ARCH\",\"dynamic\":\[\"$dir/libw.dylib\",\"/[^\"]*/libSystem.B.tbd\"\],\"weak\":\[\"$dir/libw.dylib\"\],\"archives\":\[\"$dir/libl.a\"\]\}" $t/trace
grep -qxF "{\"uuid\":\"$uuid\",\"name\":\"$t/exe\",\"arch\":\"$ARCH\",\"dynamic\":[\"/usr/lib/libSystem.B.dylib\"],\"weak\":[\"@rpath/libw.dylib\"]}" $t/trace-sc

grep -qF "{ \"version\":\"2\", \"minor-version\":1, \"name\":\"exe\", \"uuid\":\"$uuid\", \"arch\":\"$ARCH\", \"platforms\": [ { \"name\" : \"macOS\", \"min-version\" : { \"major\": " $t/trace-syms
grep -qF "\"exports\": [ ], \"linked-dylibs\":[ { \"path\": \"$dir/libw.dylib\"" $t/trace-syms
grep -qF "{ \"path\": \"$dir/libw.dylib\", \"install-name\": \"@rpath/libw.dylib\", \"arch\": \"$ARCH\", \"attributes\": [\"weak\" ], \"imported-symbols\": [ \"_w1\" ] }, { \"path\": " $t/trace-syms
# (The compiler driver adds libclang_rt's archive, of which it loads
# nothing here.)
grep -qF "\"archives\": [ \"$dir/libl.a\" ], \"unused-archives\": [ " $t/trace-syms
grep -qF "/libclang_rt.osx.a\", \"$dir/libl.a\", \"$dir/libz.a\" ],\"linked-archives\":[{ \"arch\": \"$ARCH\", \"path\": \"$dir/libl.a\",\"imported-symbols\":[\"_l1\"]}] }" $t/trace-syms

# The records are appended.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-weak_library,$t/libw.dylib $t/libl.a \
  -Wl,-trace_file,$t/trace
[ "$(wc -l < $t/trace)" -eq 2 ]

# Without a UUID, there is no trace of the dylibs.
rm -f $t/trace2
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-weak_library,$t/libw.dylib $t/libl.a \
  -Wl,-trace_file,$t/trace2 -Wl,-no_uuid
[ ! -e $t/trace2 ]

rm -f $t/exe3
not $CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-weak_library,$t/libw.dylib $t/libl.a \
  -Wl,-trace_file,$t/no/such/trace 2> $t/log
grep -q "Could not open or create trace file (errno=2): $t/no/such/trace" $t/log
[ ! -e $t/exe3 ]
