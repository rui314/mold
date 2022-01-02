#!/bin/sh
# shellcheck shell=sh
#
# Copyright (c) 2005-2021 Intel Corporation
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Get absolute path to script. Gets a relative path as argument and outputs an absolute path.
get_script_path() (
  script_path="$1"
  while [ -L "$script_path" ] ; do
    script_dir=$(command dirname -- "$script_path")
    script_dir=$(cd "$script_dir" && command pwd -P)
    script_path="$(readlink "$script_path")"
    case $script_path in
      (/*) ;;
       (*) script_path="$script_dir/$script_path" ;;
    esac
  done
  script_dir=$(command dirname -- "$script_path")
  script_dir=$(cd "$script_dir" && command pwd -P)
  printf "%s" "$script_dir"
)

_vars_get_proc_name() {
  if [ -n "${ZSH_VERSION:-}" ] ; then
    script="$(ps -p "$$" -o comm=)"
  else
    script="$1"
    while [ -L "$script" ] ; do
      script="$(readlink "$script")"
    done
  fi
  basename -- "$script"
}

_vars_this_script_name="vars.sh"
if [ "$_vars_this_script_name" = "$(_vars_get_proc_name "$0")" ] ; then
  echo ":: ERROR: Incorrect usage: this script must be sourced."
  echo "   Usage: . path/to/${_vars_this_script_name}"
  return 255 2>/dev/null || exit 255
fi

# Prepend path segment(s) to path-like env vars (PATH, CPATH, etc.).

# prepend_path() avoids dangling ":" that affects some env vars (PATH and CPATH)
# PATH > https://www.gnu.org/software/libc/manual/html_node/Standard-Environment.html

# Usage:
#   env_var=$(prepend_path "$prepend_to_var" "$existing_env_var")
#   export env_var
#
# Inputs:
#   $1 == path segment to be prepended to $2
#   $2 == value of existing path-like environment variable

prepend_path() (
  path_to_add="$1"
  path_is_now="$2"

  if [ "" = "${path_is_now}" ] ; then   # avoid dangling ":"
    printf "%s" "${path_to_add}"
  else
    printf "%s" "${path_to_add}:${path_is_now}"
  fi
)

# Extract the name and location of this sourced script.

# Generally, "ps -o comm=" is limited to a 15 character result, but it works
# fine for this usage, because we are primarily interested in finding the name
# of the execution shell, not the name of any calling script.

vars_script_name=""
vars_script_shell="$(ps -p "$$" -o comm=)"
# ${var:-} needed to pass "set -eu" checks
if [ -n "${ZSH_VERSION:-}" ] && [ -n "${ZSH_EVAL_CONTEXT:-}" ] ; then     # zsh 5.x and later
  # shellcheck disable=2249
  case $ZSH_EVAL_CONTEXT in (*:file*) vars_script_name="${(%):-%x}" ;; esac ;
elif [ -n "${KSH_VERSION:-}" ] ; then                                     # ksh, mksh or lksh
  if [ "$(set | grep -Fq "KSH_VERSION=.sh.version" ; echo $?)" -eq 0 ] ; then # ksh
    vars_script_name="${.sh.file}" ;
  else # mksh or lksh or [lm]ksh masquerading as ksh or sh
    # force [lm]ksh to issue error msg; which contains this script's path/filename, e.g.:
    # mksh: /home/ubuntu/intel/oneapi/vars.sh[137]: ${.sh.file}: bad substitution
    vars_script_name="$( (echo "${.sh.file}") 2>&1 )" || : ;
    vars_script_name="$(expr "${vars_script_name:-}" : '^.*sh: \(.*\)\[[0-9]*\]:')" ;
  fi
elif [ -n "${BASH_VERSION:-}" ] ; then        # bash
  # shellcheck disable=2128
  (return 0 2>/dev/null) && vars_script_name="${BASH_SOURCE}" ;
elif [ "dash" = "$vars_script_shell" ] ; then # dash
  # force dash to issue error msg; which contains this script's rel/path/filename, e.g.:
  # dash: 146: /home/ubuntu/intel/oneapi/vars.sh: Bad substitution
  vars_script_name="$( (echo "${.sh.file}") 2>&1 )" || : ;
  vars_script_name="$(expr "${vars_script_name:-}" : '^.*dash: [0-9]*: \(.*\):')" ;
elif [ "sh" = "$vars_script_shell" ] ; then   # could be dash masquerading as /bin/sh
  # force a shell error msg; which should contain this script's path/filename
  # sample error msg shown; assume this file is named "vars.sh"; as required by setvars.sh
  vars_script_name="$( (echo "${.sh.file}") 2>&1 )" || : ;
  if [ "$(printf "%s" "$vars_script_name" | grep -Eq "sh: [0-9]+: .*vars\.sh: " ; echo $?)" -eq 0 ] ; then # dash as sh
    # sh: 155: /home/ubuntu/intel/oneapi/vars.sh: Bad substitution
    vars_script_name="$(expr "${vars_script_name:-}" : '^.*sh: [0-9]*: \(.*\):')" ;
  fi
else  # unrecognized shell or dash being sourced from within a user's script
  # force a shell error msg; which should contain this script's path/filename
  # sample error msg shown; assume this file is named "vars.sh"; as required by setvars.sh
  vars_script_name="$( (echo "${.sh.file}") 2>&1 )" || : ;
  if [ "$(printf "%s" "$vars_script_name" | grep -Eq "^.+: [0-9]+: .*vars\.sh: " ; echo $?)" -eq 0 ] ; then # dash
    # .*: 164: intel/oneapi/vars.sh: Bad substitution
    vars_script_name="$(expr "${vars_script_name:-}" : '^.*: [0-9]*: \(.*\):')" ;
  else
    vars_script_name="" ;
  fi
fi

if [ "" = "$vars_script_name" ] ; then
  >&2 echo ":: ERROR: Unable to proceed: possible causes listed below."
  >&2 echo "   This script must be sourced. Did you execute or source this script?" ;
  >&2 echo "   Unrecognized/unsupported shell (supported: bash, zsh, ksh, m/lksh, dash)." ;
  >&2 echo "   Can be caused by sourcing from ZSH version 4.x or older." ;
  return 255 2>/dev/null || exit 255
fi

TBBROOT=$(get_script_path "${vars_script_name:-}")/..
LIBTBB_NAME="libtbb.dylib"

if [ -e "$TBBROOT/lib/$LIBTBB_NAME" ]; then
    export TBBROOT

    LIBRARY_PATH=$(prepend_path "${TBBROOT}/lib" "${LIBRARY_PATH:-}") ; export LIBRARY_PATH
    DYLD_LIBRARY_PATH=$(prepend_path "${TBBROOT}/lib" "${DYLD_LIBRARY_PATH:-}") ; export DYLD_LIBRARY_PATH
    CPATH=$(prepend_path "${TBBROOT}/include" "${CPATH:-}") ; export CPATH
    CMAKE_PREFIX_PATH=$(prepend_path "${TBBROOT}" "${CMAKE_PREFIX_PATH:-}") ; export CMAKE_PREFIX_PATH
    PKG_CONFIG_PATH=$(prepend_path "${TBBROOT}/lib/pkgconfig" "${PKG_CONFIG_PATH:-}") ; export PKG_CONFIG_PATH
else
    >&2 echo "ERROR: $LIBTBB_NAME library does not exist in $TBBROOT/lib."
    return 255 2>/dev/null || exit 255
fi
