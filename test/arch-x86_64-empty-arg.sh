#!/bin/bash
. $(dirname $0)/common.inc

nm -D mold | grep __msan_init && skip

not ./mold -m elf_x86_64 '' |& grep 'cannot open :'
