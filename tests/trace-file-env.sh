#!/bin/bash
source "$(dirname "$0")"/common.inc

# Apple's build system asks for the traces in the environment: with
# $LD_TRACE_DEPENDENTS set, -trace_file's record goes to $LD_TRACE_FILE,
# and then -trace_symbols_file's to a file of its own in
# $LD_TRACE_SYMBOLS_DIR, made as need be and named after the link's
# parent's and own process IDs, the architecture and the time. An
# option naming a file wins.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
rm -rf $t/trace $t/syms
LD_TRACE_DEPENDENTS=1 LD_TRACE_FILE=$t/trace LD_TRACE_SYMBOLS_DIR=$t/syms/sub \
  $CC --ld-path=$mold -o $t/exe $t/a.o
uuid=$(dwarfdump --uuid $t/exe | awk '{ print $2 }')
jq -e --arg uuid $uuid '.uuid == $uuid and .name == "exe"' $t/trace > /dev/null
ls $t/syms/sub > $t/log
grep -Eqx "[0-9]+\.[0-9]+\.$ARCH\.[0-9]{16}\.json" $t/log
jq -e --arg uuid $uuid '.uuid == $uuid and .name == "exe"' $t/syms/sub/*.json > /dev/null

# Without $LD_TRACE_DEPENDENTS, there is no trace.
rm -f $t/trace2
LD_TRACE_FILE=$t/trace2 $CC --ld-path=$mold -o $t/exe $t/a.o
[ ! -e $t/trace2 ]

rm -f $t/trace3 $t/trace4
LD_TRACE_DEPENDENTS=1 LD_TRACE_FILE=$t/trace3 \
  $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-trace_file,$t/trace4
[ ! -e $t/trace3 ]
[ -e $t/trace4 ]

# A -trace_file is enough for $LD_TRACE_SYMBOLS_DIR.
rm -rf $t/syms2
LD_TRACE_SYMBOLS_DIR=$t/syms2 $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-trace_file,$t/trace4
ls $t/syms2 | grep -q json

# A directory that can't be made fails the link.
touch $t/file
not env LD_TRACE_DEPENDENTS=1 LD_TRACE_FILE=$t/trace LD_TRACE_SYMBOLS_DIR=$t/file \
  $CC --ld-path=$mold -o $t/exe $t/a.o 2> $t/log2
grep -q "call to mkpath_np($t/file) failed due to: Undefined error: 0" $t/log2
