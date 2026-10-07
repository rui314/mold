#!/bin/bash
source "$(dirname "$0")"/common.inc

# "start", crt1.o's entry point, is what an LC_UNIXTHREAD image starts
# at. Where dyld calls LC_MAIN's entry point as main, -e start (the last
# -e given) is ignored with a warning, and _main stays the entry point,
# whether or not anything defines start; an image that starts from
# LC_UNIXTHREAD takes it.
cat <<EOF | $CC -o $t/a.o -c -xc -
int start(void) { return 7; }
int foo(void) { return 5; }
int main(void) { return 0; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
int main(void) { return 0; }
EOF

# The address LC_MAIN names, and a symbol's.
entry() {
  printf '0x%x\n' $((0x100000000 + $(otool -l $1 | grep -A2 'cmd LC_MAIN' |
    awk '$1 == "entryoff" { print $2 }')))
}
sym() { printf '0x%x\n' 0x$(nm $1 | awk -v s=$2 '$3 == s { print $1 }'); }

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-e,start 2> $t/log
grep -q "Ignoring '-e start' because entry point 'start' is not used" $t/log
[ $(entry $t/exe) = $(sym $t/exe _main) ]
$RUN $t/exe

$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-e,start 2> $t/log
grep -q "Ignoring '-e start'" $t/log
[ $(entry $t/exe) = $(sym $t/exe _main) ]

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-e,start,-e,_foo 2> $t/log
not grep -q "Ignoring '-e start'" $t/log
[ $(entry $t/exe) = $(sym $t/exe _foo) ]

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-e,_foo,-e,start 2> $t/log
grep -q "Ignoring '-e start'" $t/log
[ $(entry $t/exe) = $(sym $t/exe _main) ]

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-e,_start 2> $t/log
not grep -q Ignoring $t/log
[ $(entry $t/exe) = $(sym $t/exe _start) ]

# A -static image starts from LC_UNIXTHREAD.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.globl start
start: ret
EOF
$mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 15.0 15.0} -static -e start \
  -o $t/static $t/c.o 2> $t/log
not grep -q Ignoring $t/log
otool -l $t/static | grep -q LC_UNIXTHREAD
