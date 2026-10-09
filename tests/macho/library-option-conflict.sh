#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime refuses to re-export a library that it links weakly or
# lazily, whichever options name it (see dylib-named-twice.sh for which
# namings merge), and spells the pair as the option that makes it
# spells the library.
echo 'int foo(void) { return 3; }' | $CC -o $t/foo.o -c -xc -
mkdir -p $t/Foo.framework
$CC -o $t/libfoo.dylib -shared $t/foo.o -Wl,-install_name,/u/libfoo.dylib
$CC -o $t/Foo.framework/Foo -shared $t/foo.o -Wl,-install_name,/u/Foo
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

not $CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -L$t -Wl,-weak-lfoo,-reexport-lfoo 2> $t/log
grep -Fq "'-weak-lfoo' and '-reexport-lfoo' cannot be used together" $t/log

not $CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -L$t -Wl,-reexport-lfoo,-lazy-lfoo 2> $t/log
grep -Fq "'-lazy-lfoo' and '-reexport-lfoo' cannot be used together" $t/log

not $CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -F$t \
  -Wl,-weak_framework,Foo,-reexport_framework,Foo 2> $t/log
grep -Fq "'-weak_framework Foo' and '-reexport_framework Foo' cannot be used together" $t/log

not $CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -L$t \
  -Wl,-weak-lfoo,-reexport_library,$t/libfoo.dylib 2> $t/log
grep -Fq "'-weak-l$t/libfoo.dylib' and '-reexport-l$t/libfoo.dylib' cannot be used together" $t/log

not $CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -L$t \
  -Wl,-reexport_library,$t/libfoo.dylib,-weak-lfoo 2> $t/log
grep -Fq "'-weak-lfoo' and '-reexport-lfoo' cannot be used together" $t/log

# It stops at a library it doesn't find before.
not $CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -L$t \
  -Wl,-lnone,-weak-lfoo,-reexport-lfoo 2> $t/log
grep -Fq "library 'none' not found" $t/log
not grep -q 'cannot be used together' $t/log

# The other pairs go together.
$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -L$t -Wl,-upward-lfoo,-reexport-lfoo
$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -L$t -Wl,-needed-lfoo,-reexport-lfoo
$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -L$t -Wl,-weak-lfoo,-reexport_library,./$t/libfoo.dylib
