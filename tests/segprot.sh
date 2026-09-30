#!/bin/bash
source "$(dirname "$0")"/common.inc

# The segment protections -segprot sets: a segment's initial protection
# (initprot) and the most it may ever be given (maxprot). ld-prime makes
# the maximum the initial protection on arm64, where nothing may raise
# it later, and takes both as given on x86-64. The first -segprot for a
# segment wins; __LINKEDIT, which dyld reads, keeps its own.

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __MYSEG,__mysect
.quad 2
EOF
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

prot() {
  otool -l $1 | awk -v s=$2 '$1 == "segname" { seg = $2 }
    seg == s && $1 == "maxprot" { m = $2 } seg == s && $1 == "initprot" { print m + 0, $2 + 0; exit }'
}

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -Wl,-segprot,__MYSEG,rwx,r
if [ $ARCH = arm64 ]; then
  [ "$(prot $t/exe __MYSEG)" = '1 1' ]
else
  [ "$(prot $t/exe __MYSEG)" = '7 1' ]
fi

$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o -Wl,-segprot,__MYSEG,r,r \
  -Wl,-segprot,__MYSEG,rw,rw
[ "$(prot $t/exe2 __MYSEG)" = '1 1' ]

$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/a.o -Wl,-segprot,__LINKEDIT,rw,rw 2> $t/log3
grep -q -- '-segprot cannot be used to modify __LINKEDIT protections' $t/log3
[ "$(prot $t/exe3 __LINKEDIT)" = '1 1' ]
