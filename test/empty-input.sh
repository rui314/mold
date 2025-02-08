#!/bin/bash
. $(dirname $0)/common.inc

rm -f $t/a.o
touch $t/a.o
not $CC -B. -o $t/exe $t/a.o &> $t/log
grep -q 'unknown file type' $t/log
