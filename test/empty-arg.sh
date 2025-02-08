#!/bin/bash
. $(dirname $0)/common.inc

not ./mold -m elf_x86_64 '' 2>&1 | grep -q 'cannot open :'
