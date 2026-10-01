#!/bin/bash
source "$(dirname "$0")"/common.inc

# -dyld_env DYLD_xxx=value records a variable dyld sets as it launches
# the main executable (LC_DYLD_ENVIRONMENT, one per option, after the
# run paths). Here it lets dyld find a library its install name misses.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 3; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int foo();
int main() { printf("%d\n", foo()); }
EOF

mkdir -p $t/lib
$CC --ld-path=$mold -o $t/lib/libfoo.dylib -shared $t/a.o \
  -Wl,-install_name,/no/such/dir/libfoo.dylib
$CC --ld-path=$mold -o $t/exe $t/b.o $t/lib/libfoo.dylib -Wl,-rpath,/x \
  -Wl,-dyld_env,DYLD_LIBRARY_PATH=@executable_path/lib -Wl,-dyld_env,DYLD_X=a=b
otool -l $t/exe > $t/cmds
grep -A2 LC_DYLD_ENVIRONMENT $t/cmds > $t/env
grep -q 'name DYLD_LIBRARY_PATH=@executable_path/lib (offset 12)' $t/env
grep -q 'name DYLD_X=a=b (offset 12)' $t/env
grep -A1 'cmd LC_RPATH' $t/cmds | grep -q 'cmdsize 16'
awk '$1 == "cmd" { print $2 }' $t/cmds | tr '\n' ' ' > $t/order
grep -q 'LC_RPATH LC_DYLD_ENVIRONMENT LC_DYLD_ENVIRONMENT LC_FUNCTION_STARTS' $t/order
if native_arch; then
  $t/exe | grep -q '^3$'
fi

# The variable must start with DYLD_ and have a value.
for arg in FOO=bar DYLD_FOO DYLD=1; do
  not $CC --ld-path=$mold -o $t/exe2 $t/b.o $t/lib/libfoo.dylib -Wl,-dyld_env,$arg 2> $t/log
  grep -q "malformed '-dyld_env $arg', arg should be of form 'DYLD_xxx=something'" $t/log
done
not $mold -o $t/exe2 $t/b.o -dyld_env 2> $t/log
grep -q -- '-dyld_env missing <arg>' $t/log

# Only a main executable has one.
not $CC --ld-path=$mold -o $t/c.dylib -shared $t/a.o -Wl,-dyld_env,DYLD_A=1 2> $t/log
grep -q -- '-dyld_env can only used used when creating a main executables' $t/log
not $mold -r -o $t/c.o $t/a.o -dyld_env DYLD_A=1 2> $t/log
grep -q -- '-dyld_env can only used used when creating a main executables' $t/log
