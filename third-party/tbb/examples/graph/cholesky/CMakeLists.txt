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

project(cholesky CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

# TODO: Consider using FindMKL module
find_library(MKL_INTEL_LP64_LIB mkl_intel_lp64 PATHS ENV LIBRARY_PATH)
find_library(MKL_SEQUENTIAL_LIB mkl_sequential PATHS ENV LIBRARY_PATH)
find_library(MKL_CORE_LIB       mkl_core PATHS ENV LIBRARY_PATH)
if(NOT (MKL_INTEL_LP64_LIB OR MKL_SEQUENTIAL_LIB OR MKL_CORE_LIB))
    message(FATAL_ERROR "Cannot find Intel(R) Math Kernel Library (Intel(R) MKL).")
endif()

add_executable(cholesky init.cpp cholesky.cpp)
target_link_libraries(cholesky
    ${MKL_INTEL_LP64_LIB}
    ${MKL_SEQUENTIAL_LIB}
    ${MKL_CORE_LIB}
    TBB::tbb
    Threads::Threads
)
target_compile_options(cholesky PRIVATE ${TBB_CXX_STD_FLAG})

set(EXECUTABLE "$<TARGET_FILE:cholesky>")
set(ARGS 4 2)
add_execution_target(run_cholesky cholesky ${EXECUTABLE} "${ARGS}")
