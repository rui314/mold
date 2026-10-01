#!/bin/bash
source "$(dirname "$0")"/common.inc

# A document inlined in a stub is a library the stub re-exports only if
# a document lists it: one that none lists leaves its symbols undefined,
# private or public, in the YAML and the JSON format alike.
cat > $t/libw.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '/usr/lib/libw.dylib'
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ _w ]
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '$t/sub/libu.dylib'
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ _sym ]
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '/usr/lib/libv.dylib'
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ _sym2 ]
...
EOF

cat > $t/libj.tbd <<EOF
{"tapi_tbd_version":5,"main_library":{
  "target_info":[{"target":"$ARCH-macos"}],
  "install_names":[{"name":"/usr/lib/libj.dylib"}],
  "exported_symbols":[{"data":{"global":["_j"]}}]},
 "libraries":[{"target_info":[{"target":"$ARCH-macos"}],
  "install_names":[{"name":"$t/sub/libjp.dylib"}],
  "exported_symbols":[{"data":{"global":["_sym"]}}]}]}
EOF

echo 'extern int sym; int main() { return (long)&sym == 0; }' | $CC -o $t/a.o -c -xc -
echo 'extern int sym2; int main() { return (long)&sym2 == 0; }' | $CC -o $t/b.o -c -xc -
echo 'int w; int main() { return w; }' | $CC -o $t/c.o -c -xc -

not $CC --ld-path=$mold -o $t/exe1 $t/a.o -L$t -lw 2> $t/log1
grep -q _sym $t/log1
not $CC --ld-path=$mold -o $t/exe2 $t/b.o -L$t -lw 2> $t/log2
grep -q _sym2 $t/log2
not $CC --ld-path=$mold -o $t/exe3 $t/a.o -L$t -lj 2> $t/log3
grep -q _sym $t/log3

# Listing it makes it a re-export.
cat > $t/libw2.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '/usr/lib/libw2.dylib'
reexported-libraries:
  - targets:         [ $ARCH-macos ]
    libraries:       [ '$t/sub/libu.dylib' ]
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ _w ]
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '$t/sub/libu.dylib'
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ _sym ]
...
EOF
$CC --ld-path=$mold -o $t/exe4 $t/a.o -L$t -lw2
nm -m $t/exe4 | grep -q '_sym (from libw2)'
