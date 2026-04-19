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

include(CheckSymbolExists)

if (UNIX)
    set(CMAKE_REQUIRED_FLAGS -Wno-deprecated-declarations)
    if (APPLE)
        set(CMAKE_REQUIRED_DEFINITIONS -D_XOPEN_SOURCE)
    endif()

    check_symbol_exists("getcontext" "ucontext.h" _tbb_have_ucontext)
    if (NOT _tbb_have_ucontext)
        set(TBB_RESUMABLE_TASKS_USE_THREADS "__TBB_RESUMABLE_TASKS_USE_THREADS=1")
    endif()

    unset(_tbb_have_ucontext)
    unset(CMAKE_REQUIRED_DEFINITIONS)
    unset(CMAKE_REQUIRED_FLAGS)
endif()
