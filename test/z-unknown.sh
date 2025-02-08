#!/bin/bash
. $(dirname $0)/common.inc

not ./mold -z no-such-opt |& grep 'unknown command line option: -z no-such-opt'
not ./mold -zno-such-opt |& grep 'unknown command line option: -zno-such-opt'
