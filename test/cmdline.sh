#!/bin/bash
. $(dirname $0)/common.inc

not ./mold -zfoo |& grep 'unknown command line option: -zfoo'
not ./mold -z foo |& grep 'unknown command line option: -z foo'
not ./mold -abcdefg |& grep 'unknown command line option: -abcdefg'
not ./mold --abcdefg |& grep 'unknown command line option: --abcdefg'
