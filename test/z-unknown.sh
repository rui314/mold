#!/bin/bash
. $(dirname $0)/common.inc

./mold -z no-such-opt |& grep -q 'unknown command line option: -z no-such-opt'
./mold -zno-such-opt |& grep -q 'unknown command line option: -zno-such-opt'
