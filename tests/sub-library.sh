#!/bin/bash
source "$(dirname "$0")"/common.inc

# -sub_library re-exports the library whose file is so named less its
# extension, whatever its install name; -sub_umbrella the framework a
# -framework option names so. Either loads the library strongly.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 1; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
int foo();
int bar() { return foo(); }
EOF

mkdir -p $t/lib $t/Fw.framework
$CC --ld-path=$mold -o $t/lib/libfoo.A.dylib -shared $t/a.o \
  -Wl,-install_name,/usr/lib/libother.dylib
$CC --ld-path=$mold -o $t/Fw.framework/Fw -shared $t/a.o \
  -Wl,-install_name,/Library/Frameworks/Fw.framework/Fw

cmds() {
  otool -l $1 > $1.lc
  awk '$1 == "cmd" && $2 ~ /DYLIB/ { c = $2 } $1 == "name" && c { print c, $2; c = "" }' $1.lc
}

$CC --ld-path=$mold -o $t/c.dylib -shared $t/b.o -L$t/lib -lfoo.A \
  -Wl,-sub_library,libfoo.A
cmds $t/c.dylib | grep -q '^LC_REEXPORT_DYLIB /usr/lib/libother.dylib$'
$CC --ld-path=$mold -o $t/c.dylib -shared $t/b.o -L$t/lib -lfoo.A \
  -Wl,-sub_library,libfoo
cmds $t/c.dylib | grep -q '^LC_LOAD_DYLIB /usr/lib/libother.dylib$'

$CC --ld-path=$mold -o $t/c.dylib -shared $t/b.o -L$t/lib -Wl,-weak-lfoo.A \
  -Wl,-sub_library,libfoo.A 2> $t/log
grep -q 'warning: re-exported dylibs cannot be weak-linked: /usr/lib/libother.dylib' $t/log
cmds $t/c.dylib | grep -q '^LC_REEXPORT_DYLIB /usr/lib/libother.dylib$'

$CC --ld-path=$mold -o $t/c.dylib -shared $t/b.o -F$t -framework Fw -Wl,-sub_umbrella,Fw
cmds $t/c.dylib | grep -q '^LC_REEXPORT_DYLIB /Library/Frameworks/Fw.framework/Fw$'
$CC --ld-path=$mold -o $t/c.dylib -shared $t/b.o $t/Fw.framework/Fw -Wl,-sub_umbrella,Fw
cmds $t/c.dylib | grep -q '^LC_LOAD_DYLIB /Library/Frameworks/Fw.framework/Fw$'

$CC --ld-path=$mold -o $t/c.dylib -shared $t/b.o -F$t -framework Fw -Wl,-sub_library,Fw 2> $t/log
grep -q 'warning: using -sub_library to re-export a framework is deprecated.  Use -reexport_framework instead' $t/log
cmds $t/c.dylib | grep -q '^LC_REEXPORT_DYLIB /Library/Frameworks/Fw.framework/Fw$'

not $mold -o $t/c.dylib -dylib $t/b.o -sub_umbrella 2> $t/log
grep -q -- '-sub_umbrella missing <name>' $t/log
