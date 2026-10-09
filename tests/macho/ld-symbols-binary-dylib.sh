#!/bin/bash
source "$(dirname "$0")"/common.inc

# The $ld$ directives name macOS and its versions.
on_simulator && skip

# A dylib binary may carry the linker directives a stub lists among its
# exports ($ld$add, $ld$hide, $ld$install_name, $ld$previous), as
# absolute symbols it exports. ld-prime obeys them for the link's
# target there too, whether the dylib is linked or re-exported.
cat <<'EOF' | $CC -o $t/foo.o -c -xc -
int foo(void) { return 1; }
int bar(void) { return 2; }
int baz(void) { return 3; }
__asm__(".globl \"$ld$hide$os13.0$_foo\"\n\"$ld$hide$os13.0$_foo\" = 0\n"
        ".globl \"$ld$add$os13.0$_added\"\n\"$ld$add$os13.0$_added\" = 0\n"
        ".globl \"$ld$previous$/opt/old/libbaz.dylib$1.2.3$1$10.0$14.0$_baz$\"\n"
        "\"$ld$previous$/opt/old/libbaz.dylib$1.2.3$1$10.0$14.0$_baz$\" = 0\n");
EOF
$CC -shared -o $t/libfoo.dylib $t/foo.o -install_name $t/libfoo.dylib

cat <<'EOF' | $CC -o $t/rename.o -c -xc -
int bar(void) { return 2; }
__asm__(".globl \"$ld$install_name$os13.0$/opt/new/librename.dylib\"\n"
        "\"$ld$install_name$os13.0$/opt/new/librename.dylib\" = 0\n");
EOF
$CC -shared -o $t/librename.dylib $t/rename.o -install_name /opt/lib/librename.dylib

# A dylib that re-exports libfoo.
echo 'void outer(void) {}' | $CC -o $t/outer.o -c -xc -
$CC -shared -o $t/libouter.dylib $t/outer.o -install_name /opt/lib/libouter.dylib \
  -Wl,-reexport_library,$t/libfoo.dylib

cat <<EOF | $CC -o $t/a.o -c -xc -
int bar(void), baz(void), added(void);
int main() { return bar() + baz() + added(); }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
int foo(void);
int main() { return foo(); }
EOF

for lib in libfoo libouter; do
  $CC --ld-path=$mold -o $t/exe $t/a.o $t/$lib.dylib -Wl,-platform_version,macos,13.0,13.0
  dyld_info -fixups $t/exe > $t/fixups
  grep -q "$lib/_added$" $t/fixups
  grep -q "$lib/_bar$" $t/fixups
  grep -q "libbaz/_baz$" $t/fixups
  otool -L $t/exe | grep -q '/opt/old/libbaz.dylib (compatibility version 1.2.3'

  not $CC --ld-path=$mold -o $t/exe2 $t/b.o $t/$lib.dylib \
    -Wl,-platform_version,macos,13.0,13.0 2> /dev/null
  $CC --ld-path=$mold -o $t/exe3 $t/b.o $t/$lib.dylib -Wl,-platform_version,macos,12.0,12.0
done

$CC --ld-path=$mold -o $t/exe4 $t/a.o $t/libfoo.dylib $t/librename.dylib \
  -Wl,-platform_version,macos,13.0,13.0
otool -L $t/exe4 | grep -q /opt/new/librename.dylib
