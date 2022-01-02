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

include(${CMAKE_CURRENT_LIST_DIR}/../config_generation.cmake)

# TBBConfig in TBB provided packages are expected to be placed into: <tbb-root>/lib/cmake/tbb*
set(TBB_ROOT_REL_PATH "../../..")

# Paths relative to TBB root directory
set(INC_REL_PATH "include")
set(LIB_REL_PATH "lib")
set(DLL_REL_PATH "redist")

# Parse version info
file(READ ${CMAKE_CURRENT_LIST_DIR}/../../include/oneapi/tbb/version.h _tbb_version_info)
string(REGEX REPLACE ".*#define TBB_VERSION_MAJOR ([0-9]+).*" "\\1" _tbb_ver_major "${_tbb_version_info}")
string(REGEX REPLACE ".*#define TBB_VERSION_MINOR ([0-9]+).*" "\\1" _tbb_ver_minor "${_tbb_version_info}")
string(REGEX REPLACE ".*#define TBB_VERSION_PATCH ([0-9]+).*" "\\1" _tbb_ver_patch "${_tbb_version_info}")
string(REGEX REPLACE ".*#define __TBB_BINARY_VERSION ([0-9]+).*" "\\1" TBB_BINARY_VERSION "${_tbb_version_info}")
file(READ ${CMAKE_CURRENT_LIST_DIR}/../../CMakeLists.txt _tbb_cmakelist)
string(REGEX REPLACE ".*TBBMALLOC_BINARY_VERSION ([0-9]+).*" "\\1" TBBMALLOC_BINARY_VERSION "${_tbb_cmakelist}")
set(TBBMALLOC_PROXY_BINARY_VERSION ${TBBMALLOC_BINARY_VERSION})
string(REGEX REPLACE ".*TBBBIND_BINARY_VERSION ([0-9]+).*" "\\1" TBBBIND_BINARY_VERSION "${_tbb_cmakelist}")

set(COMMON_ARGS
    TBB_ROOT_REL_PATH ${TBB_ROOT_REL_PATH}
    INC_REL_PATH ${INC_REL_PATH}
    LIB_REL_PATH ${LIB_REL_PATH}
    VERSION ${_tbb_ver_major}.${_tbb_ver_minor}.${_tbb_ver_patch}
    TBB_BINARY_VERSION ${TBB_BINARY_VERSION}
    TBBMALLOC_BINARY_VERSION ${TBBMALLOC_BINARY_VERSION}
    TBBMALLOC_PROXY_BINARY_VERSION ${TBBMALLOC_PROXY_BINARY_VERSION}
    TBBBIND_BINARY_VERSION ${TBBBIND_BINARY_VERSION}
)

tbb_generate_config(INSTALL_DIR ${INSTALL_DIR}/linux   SYSTEM_NAME Linux   HANDLE_SUBDIRS ${COMMON_ARGS})
tbb_generate_config(INSTALL_DIR ${INSTALL_DIR}/windows SYSTEM_NAME Windows HANDLE_SUBDIRS DLL_REL_PATH ${DLL_REL_PATH} ${COMMON_ARGS})
tbb_generate_config(INSTALL_DIR ${INSTALL_DIR}/darwin  SYSTEM_NAME Darwin  ${COMMON_ARGS})
message(STATUS "TBBConfig files were created in ${INSTALL_DIR}")
