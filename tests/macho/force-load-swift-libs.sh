#!/bin/bash
source "$(dirname "$0")"/common.inc

# -force_load_swift_libs loads every member of an archive an auto-link
# option finds whose file name starts with "libswift" (case matters),
# as -force_load would. Archives named on the command line, and
# frameworks, load as usual.
cat <<EOF | $CC -o $t/u.o -c -xc -
int unused(void) { return 1; }
EOF
mkdir -p $t/lib
for n in libswiftFoo libSwiftBar libfoo; do
  rm -f $t/lib/$n.a
  ar rcs $t/lib/$n.a $t/u.o
done
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

for lib in swiftFoo SwiftBar foo; do
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.linker_option "-l$lib"
EOF
  $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -L$t/lib
  nm $t/exe > $t/nm1
  not grep -q _unused $t/nm1
  $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -L$t/lib -Wl,-force_load_swift_libs
  nm $t/exe > $t/nm2
  if [ $lib = swiftFoo ]; then
    grep -q _unused $t/nm2
  else
    not grep -q _unused $t/nm2
  fi
done

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t/lib -lswiftFoo -Wl,-force_load_swift_libs
nm $t/exe > $t/nm3
not grep -q _unused $t/nm3
$CC --ld-path=$mold -o $t/exe $t/a.o -L$t/lib -Wl,-add_linker_option,-lswiftFoo \
  -Wl,-force_load_swift_libs
nm $t/exe | grep -q _unused
