# Copyright (c) 2025 Intel Corporation
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

cmake_minimum_required(VERSION 3.11)
project(doc_examples_testing LANGUAGES CXX)

set(_reference_examples_path "${CMAKE_CURRENT_SOURCE_DIR}/../reference/examples")
set(_userguide_examples_path "${CMAKE_CURRENT_SOURCE_DIR}/../tbb_userguide/examples")

add_custom_target(build-doc-examples
    COMMENT "Build oneTBB documentation samples")
set(doc_examples_test_label "doc-examples")

macro(add_doc_example _doc_example_path)
    get_filename_component(_doc_example_name "${_doc_example_path}" NAME_WE)
    add_executable(${_doc_example_name} EXCLUDE_FROM_ALL "${_doc_example_path}")

    add_dependencies(${_doc_example_name} TBB::tbb TBB::tbbmalloc TBB::tbbmalloc_proxy)
    target_link_libraries(${_doc_example_name} TBB::tbb TBB::tbbmalloc TBB::tbbmalloc_proxy)

    add_dependencies(build-doc-examples ${_doc_example_name})
    add_test(NAME ${_doc_example_name} COMMAND ${_doc_example_name})
    set_tests_properties(${_doc_example_name} PROPERTIES LABELS "${doc_examples_test_label}")
endmacro()

file(GLOB_RECURSE DOC_EXAMPLES_LIST "${_reference_examples_path}/*.cpp" "${_userguide_examples_path}/*.cpp")

foreach(_doc_example_path IN LISTS DOC_EXAMPLES_LIST)
    add_doc_example(${_doc_example_path})
endforeach()
