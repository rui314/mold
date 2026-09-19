#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int foo() { return 0; }' | $CC --ld-path=$mold -dynamiclib \
  -xc - -o $t/libfoo.dylib -install_name $PWD/$t/libfoo.dylib
cat <<EOF | $CC -c -xc - -o $t/a.o
#include <stdio.h>
int foo();
int main() { puts("hello"); return foo(); }
EOF

$CC --ld-path=$mold $t/a.o $t/libfoo.dylib -o $t/exe \
  -Wl,-dead_strip,-sdk_imports,$t/imports.json
python3 - $t/imports.json $t/exe $ARCH $PWD/$t/libfoo.dylib <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
assert d['version'] == 1 and d['apiListVersion'] == 0
assert d['output'] == sys.argv[2] and d['arch'] == sys.argv[3]
assert d['platform'] == 'macOS' and d['linker']
assert d['deploymentVersion'] and d['sdkVersion']
assert d['inputs'][0]['path'] == d['output']
imports = {x['installName']: set(x['symbols']) for x in d['inputs'][0]['sdkImports']}
assert '_puts' in imports['/usr/lib/libSystem.B.dylib']
assert imports[sys.argv[4]] == {'_foo'}
EOF
