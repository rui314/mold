#!/bin/bash
source "$(dirname "$0")"/common.inc

# A relative -L or -F directory is relative to the working directory,
# even under the -syslibroot the compiler driver always passes: put
# under the SDK, -L. would search the SDK's root instead.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void);
int main() { return foo() != 3; }
EOF

mkdir -p $t/lib $t/fw/Foo.framework
echo 'int foo(void) { return 3; }' > $t/foo.c
$CC -dynamiclib -o $t/lib/libfoo.dylib $t/foo.c -install_name @rpath/libfoo.dylib
$CC -dynamiclib -o $t/fw/Foo.framework/Foo $t/foo.c \
  -install_name @rpath/Foo.framework/Foo

(cd $t/lib && $CC --ld-path=$mold -o ../exe1 ../a.o -L. -lfoo \
  -Wl,-rpath,@executable_path/lib)
$t/exe1

(cd $t/fw && $CC --ld-path=$mold -o ../exe2 ../a.o -F. -framework Foo \
  -Wl,-rpath,@executable_path/fw)
$t/exe2
