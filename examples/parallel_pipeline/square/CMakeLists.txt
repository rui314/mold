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

cmake_minimum_required(VERSION 3.1)

project(square CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(square gen_input.cpp square.cpp)

target_link_libraries(square TBB::tbb Threads::Threads)
target_compile_options(square PRIVATE ${TBB_CXX_STD_FLAG})

if (MSVC AND CMAKE_CXX_COMPILER_ID STREQUAL IntelLLVM)
    target_compile_options(square PRIVATE -D_CRT_SECURE_NO_WARNINGS)
endif()

set(EXECUTABLE "$<TARGET_FILE:square>")
set(ARGS 0 input.txt output.txt)
set(PERF_ARGS auto input.txt output.txt silent)

add_execution_target(run_square square ${EXECUTABLE} "${ARGS}")
add_execution_target(perf_run_square square ${EXECUTABLE} "${PERF_ARGS}")
