# -*- mode: sh -*-
# Helpers for the dtrace-*.sh tests, sourced after common.inc.

# dtrace_header NAME: makes $t/NAME.h from the D script $t/NAME.d, with
# dtrace -h; a host without it skips the test.
dtrace_header() {
  command -v dtrace > /dev/null || skip
  dtrace -h -s $t/$1.d -o $t/$1.h >& /dev/null || skip
}

# dof_dump FILE: describes each DOF section of an image (see dof-dump.py).
dof_dump() {
  python3 "$(dirname "$0")"/dof-dump.py "$1"
}
