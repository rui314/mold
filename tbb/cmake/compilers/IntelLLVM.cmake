# Copyright (c) 2020-2021 Intel Corporation
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

if (WIN32)
    include(${CMAKE_CURRENT_LIST_DIR}/MSVC.cmake)
else()
    include(${CMAKE_CURRENT_LIST_DIR}/Clang.cmake)
endif()

if (CMAKE_SYSTEM_PROCESSOR MATCHES "(x86|AMD64)")
    set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -mrtm -mwaitpkg)
endif()
