# Copyright (c) 2020-2023 Intel Corporation
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

cmake_minimum_required(VERSION 3.5)

project(fgbzip2 CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(fgbzip2
    blocksort.cpp
    bzlib.cpp
    compress.cpp
    crctable.cpp
    decompress.cpp
    fgbzip2.cpp
    huffman.cpp
    randtable.cpp
)

target_link_libraries(fgbzip2 TBB::tbb Threads::Threads)
target_compile_options(fgbzip2 PRIVATE ${TBB_CXX_STD_FLAG})

if (MSVC AND CMAKE_CXX_COMPILER_ID STREQUAL IntelLLVM)
    target_compile_options(fgbzip2 PRIVATE -D_CRT_SECURE_NO_WARNINGS -D_CRT_NONSTDC_NO_DEPRECATE)
endif()

if (MSVC AND (CMAKE_CXX_COMPILER_ID STREQUAL Intel OR CMAKE_CXX_COMPILER_ID STREQUAL IntelLLVM))
    if (COMMAND target_link_options)
        target_link_options(fgbzip2 PRIVATE /FORCE:MULTIPLE /INCREMENTAL:NO)
    else()
        set_target_properties(fgbzip2 PROPERTIES LINK_FLAGS /FORCE:MULTIPLE /INCREMENTAL:NO)
    endif()
endif()

set(EXECUTABLE "$<TARGET_FILE:fgbzip2>")
set(ARGS -b=9 -async "$<TARGET_FILE:fgbzip2>")

add_execution_target(run_fgbzip2 fgbzip2 ${EXECUTABLE} "${ARGS}")
add_execution_target(perf_run_fgbzip2 fgbzip2 ${EXECUTABLE} "${ARGS}")
