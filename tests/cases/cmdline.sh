#!/usr/bin/env bash
. $(dirname $0)/common.inc

not ./mold -zfoo |& grep 'unknown command line option: -zfoo'
not ./mold -z foo |& grep 'unknown command line option: -z foo'

not ./mold -w -zfoo 2> $t/log
not grep 'unknown command line option' $t/log

not ./mold --fatal-warnings -zfoo 2> $t/log
grep 'error: unknown command line option: -zfoo' $t/log

not ./mold --color-diagnostics=always -zfoo 2> $t/log
grep $'\033' $t/log
not ./mold -abcdefg |& grep 'unknown command line option: -abcdefg'
not ./mold --abcdefg |& grep 'unknown command line option: --abcdefg'
