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

project(sudoku CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(sudoku sudoku.cpp)

target_link_libraries(sudoku TBB::tbb Threads::Threads)
target_compile_options(sudoku PRIVATE ${TBB_CXX_STD_FLAG})

if (MSVC AND CMAKE_CXX_COMPILER_ID STREQUAL IntelLLVM)
    target_compile_options(sudoku PRIVATE -D_CRT_SECURE_NO_WARNINGS)
endif()

set(EXECUTABLE "$<TARGET_FILE:sudoku>")
set(ARGS 4 ${CMAKE_CURRENT_SOURCE_DIR}/input1 verbose)
set(PERF_ARGS auto ${CMAKE_CURRENT_SOURCE_DIR}/input1 silent)

add_execution_target(run_sudoku sudoku ${EXECUTABLE} "${ARGS}")
add_execution_target(perf_run_sudoku sudoku ${EXECUTABLE} "${PERF_ARGS}")
