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

option(TBB_TEST_SPEC "Generate test specification (Doxygen)" OFF)

if (TBB_TEST_SPEC)
    find_package(Doxygen REQUIRED)

    set(DOXYGEN_PREDEFINED_MACROS
      "TBB_USE_EXCEPTIONS \
      __TBB_RESUMABLE_TASKS \
      __TBB_HWLOC_PRESENT \
      __TBB_CPP17_DEDUCTION_GUIDES_PRESENT \
      __TBB_CPP17_MEMORY_RESOURCE_PRESENT \
      __TBB_CPP14_GENERIC_LAMBDAS_PRESENT"
    )

    add_custom_target(
      test_spec ALL
      COMMAND ${DOXYGEN_EXECUTABLE} ${CMAKE_CURRENT_BINARY_DIR}/Doxyfile
      COMMENT "Generating test specification with Doxygen"
      VERBATIM)
    configure_file(${CMAKE_CURRENT_SOURCE_DIR}/doc/Doxyfile.in ${CMAKE_CURRENT_BINARY_DIR}/Doxyfile @ONLY)
endif()
