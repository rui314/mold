#!/bin/bash
source "$(dirname "$0")"/common.inc

# The $ld$ directives name macOS and its versions.
on_simulator && skip

# An $ld$previous directive that names a symbol moves only that export
# to the older library, for targets in its range: the export binds to
# a library of that install name, at the directive's version or else
# the defining library's.
cat > $t/libfoo.tbd <<'EOF'
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '/usr/lib/libfoo.dylib'
current-version: 7.5
compatibility-version: 1.5
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _foo, _bar, '_x$y',
                       '$ld$previous$/usr/lib/libold.dylib$2.5$1$10.15$14.0$_foo$',
                       '$ld$previous$/usr/lib/libold2.dylib$$1$10.15$14.0$_x$y$' ]
...
EOF

cat <<'EOF' | $CC -mmacos-version-min=13.0 -o $t/a.o -c -xc -
void foo(void);
void bar(void);
void xy(void) __asm__("_x$y");
int main() { foo(); bar(); xy(); }
EOF

$CC --ld-path=$mold -mmacos-version-min=13.0 -o $t/exe1 $t/a.o $t/libfoo.tbd \
  -Wl,-map,$t/map
otool -L $t/exe1 > $t/log1
grep -Fq '/usr/lib/libfoo.dylib (compatibility version 1.5.0, current version 7.5.0)' $t/log1
grep -Fq '/usr/lib/libold.dylib (compatibility version 2.5.0, current version 2.5.0)' $t/log1
grep -Fq '/usr/lib/libold2.dylib (compatibility version 1.5.0, current version 7.5.0)' $t/log1
nm -m $t/exe1 > $t/log2
grep -Fq '_foo (from libold)' $t/log2
grep -Fq '_bar (from libfoo)' $t/log2
grep -Fq '_x$y (from libold2)' $t/log2

# -map lists the file once.
[ "$(grep -c 'libfoo.tbd$' $t/map)" -eq 1 ]

# Not so for a target past the range.
$CC --ld-path=$mold -mmacos-version-min=14.0 -o $t/exe2 $t/a.o $t/libfoo.tbd 2> /dev/null
otool -L $t/exe2 > $t/log3
not grep -q libold $t/log3
nm -m $t/exe2 | grep -Fq '_foo (from libfoo)'

# A library all of whose bound exports moved gets no load command.
cat <<'EOF' | $CC -mmacos-version-min=13.0 -o $t/b.o -c -xc -
void foo(void);
int main() { foo(); }
EOF

$CC --ld-path=$mold -mmacos-version-min=13.0 -o $t/exe3 $t/b.o $t/libfoo.tbd
otool -L $t/exe3 > $t/log4
grep -q libold.dylib $t/log4
not grep -q libfoo.dylib $t/log4

# A moved export is a weak import if the library it moved from loads
# weakly, as it does when all its own imports are weak, while the
# older library loads as its own imports say.
cat <<'EOF' | $CC -mmacos-version-min=13.0 -o $t/c.o -c -xc -
void foo(void);
void bar(void) __attribute__((weak_import));
int main() { foo(); if (bar) bar(); }
EOF

$CC --ld-path=$mold -mmacos-version-min=13.0 -o $t/exe4 $t/c.o $t/libfoo.tbd
otool -L $t/exe4 > $t/log5
grep -Fq '/usr/lib/libfoo.dylib (compatibility version 1.5.0, current version 7.5.0, weak)' $t/log5
grep -Fq '/usr/lib/libold.dylib (compatibility version 2.5.0, current version 2.5.0)' $t/log5
nm -m $t/exe4 | grep -Fq 'weak external _foo (from libold)'
