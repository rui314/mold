#!/bin/bash
. $(dirname $0)/common.inc

# Any argument starting with '@' names a response file, whose arguments
# take its place, an option's argument too - but a dylib path starting
# @rpath, @loader_path or @executable_path. A response file splits at
# white space, quotes and backslashes as a shell's words do, and may
# name others in turn.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
link() { $CC --ld-path=$mold -o $t/exe $t/a.o "$@"; }

mkdir -p $t/rpath
echo '-dead_strip' > $t/rpath/x
(cd $t && $CC --ld-path=$mold -o exe a.o -Wl,-rpath,@rpath/x) 2> $t/log
not grep -F 'response file' $t/log
otool -l $t/exe | grep -F 'path @rpath/x '

echo "$t/rpath" > $t/rsp0
link -Wl,-rpath,@$t/rsp0
otool -l $t/exe | grep -F "path $t/rpath "

echo "@$t/rsp3" > $t/rsp2
echo '-dead_strip' > $t/rsp3
link -Wl,@$t/rsp2

cat <<EOF | $CC -o $t/b.o -c -xc -
int foo_bar() { return 0; }
int main() { return 0; }
EOF
printf -- "-exported_symbol\t'_foo_bar'\n" > $t/rsp4
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,@$t/rsp4
nm -gU $t/exe > $t/syms
grep -q ' _foo_bar$' $t/syms
not grep -q ' _main$' $t/syms
printf -- '-exported_symbol "_foo"_ba\\r' > $t/rsp5
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,@$t/rsp5
grep -q ' _foo_bar$' <(nm -gU $t/exe)

# A response file may be named twice. One that can't be read, one that
# names itself, an open quote or a backslash at the end, and a NUL byte,
# which no argument may hold, are errors. (ld-prime refuses a file named
# twice, and goes on in each of the other cases, warning of a file it
# can't open.)
if is_mold; then
  link -Wl,@$t/rsp2,@$t/rsp3
  not link -Wl,@$t/none 2> $t/log
  grep -qF "$t/none" $t/log
  not link -Wl,@$t/rpath 2> $t/log
  grep -qF "$t/rpath" $t/log
  echo "@$t/rsp6" > $t/rsp6
  not link -Wl,@$t/rsp6 2> $t/log
  grep -q 'response file nesting too deep' $t/log
  printf -- "-exported_symbol '_foo_bar" > $t/rsp7
  not $CC --ld-path=$mold -o $t/exe $t/b.o -Wl,@$t/rsp7 2> $t/log
  grep -q 'premature end of input' $t/log
  printf -- '-exported_symbol _foo_bar\\' > $t/rsp8
  not $CC --ld-path=$mold -o $t/exe $t/b.o -Wl,@$t/rsp8 2> $t/log
  grep -q 'premature end of input' $t/log
  printf -- '-exported_symbol _foo_bar\0-dead_strip' > $t/rsp9
  not $CC --ld-path=$mold -o $t/exe $t/b.o -Wl,@$t/rsp9 2> $t/log
  grep -q 'response file contains a NUL byte' $t/log
fi
