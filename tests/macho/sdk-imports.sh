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
python3 - $t/imports.json $t/exe $ARCH $PWD/$t/libfoo.dylib $PLATFORM_NAME <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
assert d['version'] == 1 and d['apiListVersion'] == 0
assert d['output'] == sys.argv[2] and d['arch'] == sys.argv[3]
assert d['platform'] == sys.argv[5] and d['linker']
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

# Of a key given twice, the last counts.
echo '{"version": 1, "version": 4, "apis": ["_puts"]}' > $t/apis5.json
$CC --ld-path=$mold $t/a.o $t/libfoo.dylib -o $t/exe5 \
  -Wl,-sdk_imports,$t/imports5.json,-sdk_imports_api_list,$t/apis5.json
jq -e '.apiListVersion == 4' $t/imports5.json > /dev/null

# A list must be JSON, with a version and APIs. (ld-prime reads it with
# NSJSONSerialization, which allows a comma before a closing bracket.)
if is_mold; then
  echo '{"version": 1, "apis": ["_puts",],}' > $t/apis6.json
  not $mold -arch $ARCH -o $t/exe4 $t/a.o -sdk_imports_api_list $t/apis6.json 2> $t/log
  grep -q "invalid list at $t/apis6.json" $t/log
fi
echo '{"apis": ["_puts"]}' > $t/apis3.json
not $mold -arch $ARCH -o $t/exe4 $t/a.o -sdk_imports_api_list $t/apis3.json 2> $t/log
grep -q "invalid list at $t/apis3.json" $t/log
echo '{"version": 1}' > $t/apis4.json
not $mold -arch $ARCH -o $t/exe4 $t/a.o -sdk_imports_api_list $t/apis4.json 2> $t/log
grep -q "invalid list at $t/apis4.json" $t/log

# The libraries are in the order of the image's load commands.
python3 - $t/imports.json $t/exe <<'EOF2'
import json, subprocess, sys
d = json.load(open(sys.argv[1]))
names = [x['installName'] for x in d['inputs'][0]['sdkImports']]
loads = subprocess.run(['otool', '-L', sys.argv[2]], capture_output=True, text=True).stdout
order = [l.split(' (')[0].strip() for l in loads.splitlines()[1:]]
assert names == [n for n in order if n in names], (names, order)
EOF2
