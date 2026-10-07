#!/bin/bash
source "$(dirname "$0")"/common.inc

# A stub lists an Objective-C class by name for its class and metaclass
# objects (objc-classes) and for its exception type (objc-eh-types).
# tapi has a class listed for its exception type alone export all three
# symbols, and so does ld-prime link them.
cat <<EOF | $CC -o $t/a.o -c -xc -
extern char cls[] __asm__("_OBJC_CLASS_\$_Fake");
extern char meta[] __asm__("_OBJC_METACLASS_\$_Fake");
extern char eh[] __asm__("_OBJC_EHTYPE_\$_Fake");
void *p[] = { cls, meta, eh };
int main() {}
EOF

cat > $t/libv3.tbd <<EOF
--- !tapi-tbd-v3
archs:           [ x86_64, arm64 ]
platform:        macosx
install-name:    '/usr/lib/libv3.dylib'
exports:
  - archs:           [ x86_64, arm64 ]
    objc-eh-types:   [ Fake ]
...
EOF
cat > $t/libv4.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-$PLATFORM, arm64-$PLATFORM ]
install-name:    '/usr/lib/libv4.dylib'
exports:
  - targets:         [ x86_64-$PLATFORM, arm64-$PLATFORM ]
    objc-eh-types:   [ Fake ]
...
EOF
cat > $t/libv5.tbd <<EOF
{
  "main_library": {
    "install_names": [{"name": "/usr/lib/libv5.dylib"}],
    "target_info": [{"target": "arm64-macos"}, {"target": "x86_64-macos"}],
    "exported_symbols": [{"data": {"objc_eh_type": ["Fake"]}}]
  },
  "tapi_tbd_version": 5
}
EOF

for v in v3 v4 v5; do
  $CC --ld-path=$mold -o $t/exe-$v $t/a.o $t/lib$v.tbd
  dyld_info -fixups $t/exe-$v > $t/fixups-$v
  for sym in CLASS METACLASS EHTYPE; do
    grep -q "lib$v/_OBJC_${sym}_\$_Fake" $t/fixups-$v
  done
done
