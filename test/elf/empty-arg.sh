#!/bin/bash
. $(dirname $0)/common.inc

! ./mold -m elf_x86_64 '' >& $t/log
grep -q 'cannot open :' $t/log
