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

project(tbb_examples CXX)

add_custom_target(build_examples)
add_custom_target(run_examples)
add_dependencies(run_examples build_examples)

add_custom_target(light_test_examples)
add_dependencies(light_test_examples build_examples)

macro(tbb_add_example subdir name)
    add_subdirectory(${subdir}/${name})
    add_dependencies(build_examples ${name})
    add_dependencies(run_examples run_${name})
    if (TARGET light_test_${name})
        add_dependencies(light_test_examples light_test_${name})
    else()
        add_dependencies(light_test_examples run_${name})
    endif()
endmacro()

tbb_add_example(concurrent_hash_map count_strings)
tbb_add_example(concurrent_priority_queue shortpath)

tbb_add_example(getting_started sub_string_finder)

tbb_add_example(graph binpack)

# TODO: Consider using FindMKL module
find_library(MKL_INTEL_LP64_LIB mkl_intel_lp64 PATHS ENV LIBRARY_PATH)
find_library(MKL_SEQUENTIAL_LIB mkl_sequential PATHS ENV LIBRARY_PATH)
find_library(MKL_CORE_LIB       mkl_core PATHS ENV LIBRARY_PATH)
if(MKL_INTEL_LP64_LIB AND MKL_SEQUENTIAL_LIB AND MKL_CORE_LIB)
    tbb_add_example(graph cholesky)
else()
    message(WARNING "Intel(R) Math Kernel Library (Intel(R) MKL) libraries were not found, graph/cholesky example is excluded from the build.")
endif()

tbb_add_example(graph dining_philosophers)
tbb_add_example(graph fgbzip2)
tbb_add_example(graph logic_sim)
tbb_add_example(graph som)

tbb_add_example(parallel_for game_of_life)
tbb_add_example(parallel_for polygon_overlay)
tbb_add_example(parallel_for seismic)
tbb_add_example(parallel_for tachyon)

tbb_add_example(parallel_for_each parallel_preorder)

tbb_add_example(parallel_pipeline square)

tbb_add_example(parallel_reduce convex_hull)
tbb_add_example(parallel_reduce primes)

tbb_add_example(task_arena fractal)

tbb_add_example(task_group sudoku)

tbb_add_example(test_all fibonacci)
