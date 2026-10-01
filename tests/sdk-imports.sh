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

# A -r link takes the option, and writes no report.
$mold -arch $ARCH -r -o $t/r.o $t/a.o -sdk_imports $t/r.json
[ ! -e $t/r.json ]

# -sdk_imports_api_list names the APIs the report lists, of the imports,
# in a JSON object ld-prime reads as it reads the option: the version,
# which the report records, and the APIs. An image with none to report
# has no input in the report.
echo '{"version": 7, "apis": ["_puts", "_nosuch"]}' > $t/apis.json
$CC --ld-path=$mold $t/a.o $t/libfoo.dylib -o $t/exe2 \
  -Wl,-sdk_imports,$t/imports2.json,-sdk_imports_api_list,$t/apis.json
python3 - $t/imports2.json <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
assert d['apiListVersion'] == 7
imports = {x['installName']: set(x['symbols']) for x in d['inputs'][0]['sdkImports']}
assert imports == {'/usr/lib/libSystem.B.dylib': {'_puts'}}
EOF

echo '{"version": "3", "apis": ["_nosuch"]}' > $t/apis2.json
$CC --ld-path=$mold $t/a.o $t/libfoo.dylib -o $t/exe3 \
  -Wl,-sdk_imports,$t/imports3.json,-sdk_imports_api_list,$t/apis2.json
python3 - $t/imports3.json <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
assert d['apiListVersion'] == 3 and d['inputs'] == []
EOF

echo '{"apis": ["_puts"]}' > $t/apis3.json
not $mold -arch $ARCH -o $t/exe4 $t/a.o -sdk_imports_api_list $t/apis3.json 2> $t/log
grep -q "invalid list at $t/apis3.json: Map node doesn't have element for key 'version'" $t/log
echo '{"version": 1}' > $t/apis4.json
not $mold -arch $ARCH -o $t/exe4 $t/a.o -sdk_imports_api_list $t/apis4.json 2> $t/log
grep -q "invalid list at $t/apis4.json: API symbol list $t/apis4.json can't be empty" $t/log
