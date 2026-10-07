#!/bin/bash
source "$(dirname "$0")"/common.inc

# The ad-hoc signature, laid out as ld-prime does: the code directory's
# page hashes follow the NUL-terminated identifier directly (88 fixed
# bytes, then the identifier), the superblob is exactly its header,
# index and code directory, LC_CODE_SIGNATURE covers that rounded up to
# 8 bytes, and the executable segment's limit is the size of
# __TEXT,__text - 0 in an image without code.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
echo 'int data = 1;' | $CC -o $t/b.o -c -xc -

mkdir -p $t/out
$CC --ld-path=$mold -o $t/out/abcdefg $t/a.o -Wl,-adhoc_codesign
$CC --ld-path=$mold -o $t/out/libdata.dylib -shared $t/b.o -Wl,-adhoc_codesign
$RUN $t/out/abcdefg

field() { codesign -d -vvvvv $1 2>&1 | sed -n "s/^$2//p"; }
cd_size() { codesign -d -vvvvv $1 2>&1 | sed -n 's/^CodeDirectory .* size=\([0-9]*\) .*/\1/p'; }
nhashes() { codesign -d -vvvvv $1 2>&1 | sed -n 's/.* hashes=\([0-9]*\)+0 .*/\1/p'; }
datasize() { otool -l $1 | grep -A3 LC_CODE_SIGNATURE | awk '/datasize/ { print $2 }'; }

exe=$t/out/abcdefg
[ "$(cd_size $exe)" = $((88 + 8 + 32 * $(nhashes $exe))) ]
[ "$(datasize $exe)" = $(( (20 + $(cd_size $exe) + 7) / 8 * 8 )) ]
[ "$(field $exe 'Executable Segment limit=')" = "$(printf '%d' $(otool -l $exe | \
  grep -A4 'sectname __text' | awk '/ size/ { print $2 }'))" ]

lib=$t/out/libdata.dylib
[ "$(cd_size $lib)" = $((88 + 14 + 32 * $(nhashes $lib))) ]
# (codesign prints no executable segment for a zero limit.)
codesign -d -vvvvv $lib > $t/libsig 2>&1
not grep -q 'Executable Segment limit=[1-9]' $t/libsig

# The identifier is the leaf name of the install name: -install_name's
# (an executable's too), else -final_output's, else the output's.
ident() { $CC --ld-path=$mold -o $t/out/x -Wl,-adhoc_codesign "$@"; field $t/out/x Identifier=; }
[ "$(ident -shared $t/b.o -Wl,-install_name,/usr/lib/libfoo.dylib)" = libfoo.dylib ]
[ "$(ident -shared $t/b.o -Wl,-install_name,@rpath/Foo.framework/Foo)" = Foo ]
[ "$(ident -shared $t/b.o -Wl,-final_output,/tmp/libbar.dylib)" = libbar.dylib ]
[ "$(ident $t/a.o -Wl,-install_name,/usr/lib/libbaz.dylib)" = libbaz.dylib ]
[ "$(ident $t/a.o -Wl,-final_output,/tmp/qux)" = qux ]
[ "$(ident $t/a.o)" = x ]
