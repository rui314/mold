#!/bin/sh
# shellcheck shell=sh
#
# Copyright (c) 2023 Intel Corporation
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

if [ -z "${SETVARS_CALL:-}" ] ; then
  >&2 echo " "
  >&2 echo ":: ERROR: This script must be sourced by setvars.sh."
  >&2 echo "   Try 'source <install-dir>/setvars.sh --help' for help."
  >&2 echo " "
  return 255
fi

if [ -z "${ONEAPI_ROOT:-}" ] ; then
  >&2 echo " "
  >&2 echo ":: ERROR: This script requires that the ONEAPI_ROOT env variable is set."
  >&2 echo "   Try 'source <install-dir>\setvars.sh --help' for help."
  >&2 echo " "
  return 254
fi

TBBROOT="${ONEAPI_ROOT}"; export TBBROOT
