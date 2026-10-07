#!/usr/bin/env bash
. $(dirname $0)/common.inc

not ./mold -zfoo |& grep 'unknown command line option: -zfoo'
not ./mold -z foo |& grep 'unknown command line option: -z foo'
# -abcdefg is -a bcdefg, and GNU ld rejects that keyword.
not ./mold -abcdefg |& grep 'unrecognized -a option .bcdefg'
not ./mold --abcdefg |& grep 'unknown command line option: --abcdefg'
