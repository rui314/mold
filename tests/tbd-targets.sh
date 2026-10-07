#!/usr/bin/env bash
. $(dirname $0)/common.inc

case $ARCH in
  arm64) other=x86_64 ;;
  x86_64) other=arm64 ;;
esac

cat > $t/v4.tbd <<EOF
--- !tapi-tbd
tbd-version: 4
targets: [ arm64-$PLATFORM, x86_64-$PLATFORM, $ARCH-ios ]
install-name: /usr/lib/libtarget.dylib
exports:
  - targets: [ $ARCH-$PLATFORM ]
    symbols: [ _right ]
  - symbols: [ _wrong_arch ]
    targets: [ $other-$PLATFORM ]
  - targets: [ $ARCH-ios ]
    symbols: [ _wrong_platform ]
...
EOF
cat > $t/v5.tbd <<EOF
{"tapi_tbd_version":5,"main_library":{
 "target_info":[{"target":"arm64-$PLATFORM"},{"target":"x86_64-$PLATFORM"},{"target":"$ARCH-ios"}],
 "install_names":[{"name":"/usr/lib/libtarget.dylib"}],
 "exported_symbols":[
  {"targets":["$ARCH-$PLATFORM"],"text":{"global":["_right"]}},
  {"targets":["$other-$PLATFORM"],"text":{"global":["_wrong_arch"]}},
  {"targets":["$ARCH-ios"],"text":{"global":["_wrong_platform"]}}
 ]}}
EOF
for version in 4 5; do
  echo 'int right(); int main() { return right(); }' | $CC -c -xc - -o $t/right.o
  $CC --ld-path=$mold $t/right.o $t/v$version.tbd -o $t/right
  for name in wrong_arch wrong_platform; do
    echo "int $name(); int main() { return $name(); }" | $CC -c -xc - -o $t/wrong.o
    ! $CC --ld-path=$mold $t/wrong.o $t/v$version.tbd -o $t/wrong 2> $t/log || false
    grep -q "undefined symbol: .*_$name" $t/log
  done
done
