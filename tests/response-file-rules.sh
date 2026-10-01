#!/bin/bash
. $(dirname $0)/common.inc

# ld-prime reads any argument starting with '@' as a response file, an
# option's argument too, but a dylib path's @rpath, @loader_path or
# @executable_path. One it can't open draws a warning, the argument
# staying as it is; one it can't read is an error, and so is reading
# one twice, nested or not, by its real path.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
link() { $CC --ld-path=$mold -o $t/exe $t/a.o "$@"; }
dir=$(cd $t && pwd -P)

link -Wl,-rpath,@$t/none 2> $t/log
grep -F "warning: response file '$t/none' could not be opened, errno=2 (No such file or directory)" $t/log
otool -l $t/exe | grep -F "path @$t/none "

mkdir -p $t/rpath
echo '-dead_strip' > $t/rpath/x
(cd $t && $CC --ld-path=$mold -o exe a.o -Wl,-rpath,@rpath/x) 2> $t/log
not grep -F 'response file' $t/log
otool -l $t/exe | grep -F 'path @rpath/x '

not link -Wl,@$t/rpath 2> $t/log
grep -F "response file '$dir/rpath' could not be read, errno=21 (Is a directory)" $t/log

echo '-dead_strip' > $t/rsp1
not link -Wl,@$t/rsp1,@$t/rsp1 2> $t/log
grep -F "recursively loading $dir/rsp1" $t/log

echo "@$t/rsp3" > $t/rsp2
echo '-dead_strip' > $t/rsp3
link -Wl,@$t/rsp2
not link -Wl,@$t/rsp2,@$t/rsp3 2> $t/log
grep -F "recursively loading $dir/rsp3" $t/log

# Arguments split at spaces, tabs, newlines and carriage returns alone;
# a quote left open or a backslash at the end ends with the file, and
# so does a NUL byte.
cat <<EOF | $CC -o $t/b.o -c -xc -
int foo_bar() { return 0; }
int main() { return 0; }
EOF
printf -- "-u\v_foo_bar" > $t/rsp4
not $CC --ld-path=$mold -o $t/exe $t/b.o -Wl,@$t/rsp4 2> $t/log
grep -F 'unknown option' $t/log
printf -- "-exported_symbol '_foo_bar" > $t/rsp5
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,@$t/rsp5
printf -- '-exported_symbol _foo_ba\\r\\' > $t/rsp6
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,@$t/rsp6
printf -- '-exported_symbol _foo_bar\0-unknown' > $t/rsp7
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,@$t/rsp7
