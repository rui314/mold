#!/bin/bash
. $(dirname $0)/common.inc

not ./mold -zfoo |& grep -q 'unknown command line option: -zfoo'
not ./mold -z foo |& grep -q 'unknown command line option: -z foo'
not ./mold -abcdefg |& grep -q 'unknown command line option: -abcdefg'
not ./mold --abcdefg |& grep -q 'unknown command line option: --abcdefg'
