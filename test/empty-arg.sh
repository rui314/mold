#!/bin/bash
. $(dirname $0)/common.inc

not ./mold -m elf_x86_64 '' |& grep 'cannot open :'
