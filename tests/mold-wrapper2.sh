#!/usr/bin/env bash
. $(dirname $0)/common.inc

nm mold | grep '__[at]san_init' && skip

# The wrapper is passed in a sealed memfd, which cannot be written to.
./mold -run bash -c 'echo $MOLD_WRAPPER_FD' | grep -E '^[0-9]+$'
not ./mold -run bash -c 'eval "echo foo >&$MOLD_WRAPPER_FD"'

# If a process closes the descriptor, the programs it runs get a new copy.
./mold -run bash -c 'eval "exec $MOLD_WRAPPER_FD<&-"; bash -c "/usr/bin/ld --version"' |
  grep mold

# If another file takes the descriptor number, the programs that the
# process runs don't preload anything from it.
./mold -run bash -c 'eval "exec $MOLD_WRAPPER_FD</dev/null"; env' >& $t/log
not grep -E '^MOLD_WRAPPER_FD=|/proc/self/fd|^LD_PRELOAD_FDS=.|ld\.so' $t/log
