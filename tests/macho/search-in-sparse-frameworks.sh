#!/bin/bash
source "$(dirname "$0")"/common.inc

# A sparse framework has its Versions/Current but not the symlinks at
# its top, Foo.framework/Foo among them, by which the search path finds
# a framework. -search_in_sparse_frameworks makes ld-prime look in each
# framework's Versions/Current too, in a second pass over the search
# path once the first has found nothing.
rm -rf $t/fw1 $t/fw2
mkdir -p $t/fw1/Foo.framework/Versions/A $t/fw2/Foo.framework/Versions/A
echo 'int foo() { return 1; }' | $CC -o $t/a.o -c -xc -
$CC --ld-path=$mold -o $t/fw1/Foo.framework/Versions/A/Foo -shared $t/a.o \
  -Wl,-install_name,/Library/Frameworks/Foo.framework/Versions/A/Foo
ln -sfn A $t/fw1/Foo.framework/Versions/Current
$CC --ld-path=$mold -o $t/fw2/Foo.framework/Versions/A/Foo -shared $t/a.o \
  -Wl,-install_name,/fw2/Foo
ln -sfn A $t/fw2/Foo.framework/Versions/Current

cat <<EOF | $CC -o $t/b.o -c -xc -
int foo();
int main() { return foo() - 1; }
EOF

not $CC --ld-path=$mold -o $t/exe $t/b.o -F$t/fw1 -framework Foo 2> $t/log
grep -q "framework 'Foo' not found" $t/log

$CC --ld-path=$mold -o $t/exe $t/b.o -F$t/fw1 -framework Foo -Wl,-search_in_sparse_frameworks
otool -L $t/exe | grep -q /Library/Frameworks/Foo.framework/Versions/A/Foo

# A framework with its top-level symlink, further on the path, wins.
ln -sf Versions/Current/Foo $t/fw2/Foo.framework/Foo
$CC --ld-path=$mold -o $t/exe $t/b.o -F$t/fw1 -F$t/fw2 -framework Foo \
  -Wl,-search_in_sparse_frameworks
otool -L $t/exe | grep -q /fw2/Foo
