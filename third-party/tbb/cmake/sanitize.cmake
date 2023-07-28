# Copyright (c) 2020-2022 Intel Corporation
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

set(TBB_SANITIZE ${TBB_SANITIZE} CACHE STRING "Sanitizer parameter passed to compiler/linker" FORCE)
# Possible values of sanitizer parameter for cmake-gui for convenience, user still can use any other value.
set_property(CACHE TBB_SANITIZE PROPERTY STRINGS "thread" "memory" "leak" "address -fno-omit-frame-pointer")

if (NOT TBB_SANITIZE)
    return()
endif()

set(TBB_SANITIZE_OPTION -fsanitize=${TBB_SANITIZE})

# It is required to add sanitizer option to CMAKE_REQUIRED_LIBRARIES to make check_cxx_compiler_flag working properly:
# sanitizer option should be passed during the compilation phase as well as during the compilation.
set(CMAKE_REQUIRED_LIBRARIES "${TBB_SANITIZE_OPTION} ${CMAKE_REQUIRED_LIBRARIES}")

string(MAKE_C_IDENTIFIER ${TBB_SANITIZE_OPTION} FLAG_DISPLAY_NAME)
check_cxx_compiler_flag(${TBB_SANITIZE_OPTION} ${FLAG_DISPLAY_NAME})
if (NOT ${FLAG_DISPLAY_NAME})
    message(FATAL_ERROR
    "${TBB_SANITIZE_OPTION} is not supported by compiler ${CMAKE_CXX_COMPILER_ID}:${CMAKE_CXX_COMPILER_VERSION}, "
    "please try another compiler or omit TBB_SANITIZE variable")
endif()

set(TBB_TESTS_ENVIRONMENT ${TBB_TESTS_ENVIRONMENT}
    "TSAN_OPTIONS=suppressions=${CMAKE_CURRENT_SOURCE_DIR}/cmake/suppressions/tsan.suppressions"
    "LSAN_OPTIONS=suppressions=${CMAKE_CURRENT_SOURCE_DIR}/cmake/suppressions/lsan.suppressions")

set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} ${TBB_SANITIZE_OPTION}")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} ${TBB_SANITIZE_OPTION}")
set(CMAKE_EXE_LINKER_FLAGS "${CMAKE_EXE_LINKER_FLAGS} ${TBB_SANITIZE_OPTION}")
