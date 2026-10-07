#!/usr/bin/env bash
. $(dirname $0)/common.inc

not ./mold -zfoo |& grep 'unknown command line option: -zfoo'
not ./mold -z foo |& grep 'unknown command line option: -z foo'
# -abcdefg is -a bcdefg, and GNU ld rejects that keyword; the built-in
# parser does not know -a yet, and rejects the word as unknown.
not ./mold -abcdefg |& grep -e 'unrecognized -a option .bcdefg' -e 'unknown command line option: -abcdefg'
not ./mold --abcdefg |& grep 'unknown command line option: --abcdefg'
