#!/bin/bash
source "$(dirname "$0")"/common.inc

# A .tbd stub stands for a dylib at link time, and the program then runs
# against the dylib itself. TAPI has written five versions of the
# format: in YAML, versions 1 (untagged), 2 and 3 give architectures
# and a platform, version 4 targets; version 5 is JSON. They spell weak
# definitions and re-exported libraries each their own way, and from
# version 3 on inline a re-exported library as a document of its own.
dir=$(cd $t && pwd -P)

cat <<EOF | $CC -o $t/qux.o -c -xc -
int qux() { return 7; }
EOF
$CC --ld-path=$mold -shared -o $t/libqux.dylib $t/qux.o \
  -Wl,-install_name,$dir/libqux.dylib

cat <<EOF | $CC -o $t/foo.o -c -xc -
int foo() { return 3; }
int bar = 4;
__attribute__((weak)) int baz() { return 5; }
_Thread_local int tls = 6;
EOF
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/foo.o -L$t -Wl,-reexport-lqux \
  -Wl,-install_name,$dir/libfoo.dylib \
  -Wl,-current_version,2.1 -Wl,-compatibility_version,1.5

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int foo();
extern int bar;
int baz();
extern _Thread_local int tls;
int qux();
int main() { printf("%d %d %d %d %d\n", foo(), bar, baz(), tls, qux()); }
EOF

# legacy <tag> <clients key>: a version 1-3 stub of libfoo.
legacy() {
  cat <<EOF
$1
archs:           [ $ARCH ]
platform:        macosx
install-name:    '$dir/libfoo.dylib'
current-version: 2.1
compatibility-version: 1.5
exports:
  - archs:           [ $ARCH ]
    $2: [ Friend ]
    re-exports:      [ '$dir/libqux.dylib' ]
    symbols:         [ _foo, _bar ]
    weak-def-symbols: [ _baz ]
    thread-local-symbols: [ _tls ]
EOF
}

{ legacy --- allowed-clients; echo ...; } > $t/v1.tbd
{ legacy '--- !tapi-tbd-v2' allowable-clients; echo ...; } > $t/v2.tbd
legacy '--- !tapi-tbd-v3' allowable-clients > $t/v3.tbd
cat >> $t/v3.tbd <<EOF
--- !tapi-tbd-v3
archs:           [ $ARCH ]
platform:        macosx
install-name:    '$dir/libqux.dylib'
exports:
  - archs:           [ $ARCH ]
    symbols:         [ _qux ]
...
EOF

cat > $t/v4.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '$dir/libfoo.dylib'
current-version: 2.1
compatibility-version: 1.5
allowable-clients:
  - targets:         [ $ARCH-macos ]
    clients:         [ Friend ]
reexported-libraries:
  - targets:         [ $ARCH-macos ]
    libraries:       [ '$dir/libqux.dylib' ]
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ _foo, _bar ]
    weak-symbols:    [ _baz ]
    thread-local-symbols: [ _tls ]
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '$dir/libqux.dylib'
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ _qux ]
...
EOF

cat > $t/v5.tbd <<EOF
{
  "tapi_tbd_version": 5,
  "main_library": {
    "target_info": [{"target": "$ARCH-macos"}],
    "install_names": [{"name": "$dir/libfoo.dylib"}],
    "current_versions": [{"version": "2.1"}],
    "compatibility_versions": [{"version": "1.5"}],
    "allowable_clients": [{"clients": ["Friend"]}],
    "reexported_libraries": [{"names": ["$dir/libqux.dylib"]}],
    "exported_symbols": [{"text": {"global": ["_foo"], "weak": ["_baz"]},
                          "data": {"global": ["_bar"], "thread_local": ["_tls"]}}]
  },
  "libraries": [{
    "target_info": [{"target": "$ARCH-macos"}],
    "install_names": [{"name": "$dir/libqux.dylib"}],
    "exported_symbols": [{"text": {"global": ["_qux"]}}]
  }]
}
EOF

for v in v1 v2 v3 v4 v5; do
  not $CC --ld-path=$mold -o $t/exe-$v $t/main.o $t/$v.tbd 2> $t/log-$v
  grep -q 'not an allowed client' $t/log-$v
  $CC --ld-path=$mold -o $t/exe-$v $t/main.o $t/$v.tbd -Wl,-client_name,Friend
  $t/exe-$v | grep -q '^3 4 5 6 7$'
  otool -L $t/exe-$v > $t/libs-$v
  grep -q "$dir/libfoo.dylib (compatibility version 1.5.0, current version 2.1.0)" $t/libs-$v
  dyld_info -fixups $t/exe-$v | grep -q 'libfoo/_qux'
done
