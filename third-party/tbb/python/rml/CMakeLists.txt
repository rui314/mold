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

add_library(irml 
    ../../src/tbb/allocator.cpp
    ../../src/tbb/dynamic_link.cpp
    ../../src/tbb/misc.cpp
    ../../src/tbb/misc_ex.cpp
    ../../src/tbb/exception.cpp
    ipc_server.cpp
    ipc_utils.cpp
)
add_library(TBB::irml ALIAS irml)

set_target_properties(irml PROPERTIES VERSION 1)

target_include_directories(irml
    PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/../../include>
    $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>)

target_compile_options(irml
    PRIVATE
    ${TBB_CXX_STD_FLAG} # TODO: consider making it PUBLIC.
    ${TBB_MMD_FLAG}
    ${TBB_DSE_FLAG}
    ${TBB_WARNING_LEVEL}
    ${TBB_LIB_COMPILE_FLAGS}
    ${TBB_COMMON_COMPILE_FLAGS}
)

target_compile_definitions(irml
                           PUBLIC
                           $<$<CONFIG:DEBUG>:TBB_USE_DEBUG>)

# Prefer using target_link_options instead of target_link_libraries to specify link options because
# target_link_libraries may incorrectly handle some options (on Windows, for example).
if (COMMAND target_link_options)
    target_link_options(irml
        PRIVATE
        ${TBB_LIB_LINK_FLAGS}
        ${TBB_COMMON_LINK_FLAGS}
    )
else()
    target_link_libraries(irml
        PRIVATE
        ${TBB_LIB_LINK_FLAGS}
        ${TBB_COMMON_LINK_FLAGS}
    )
endif()

target_link_libraries(irml
    PRIVATE
    Threads::Threads
    ${TBB_LIB_LINK_LIBS}
    ${TBB_COMMON_LINK_LIBS}
)

if (DEFINED TBB_SIGNTOOL)
    string(REPLACE " " ";" TBB_SIGNTOOL_ARGS "${TBB_SIGNTOOL_ARGS}")
    add_custom_command(TARGET irml POST_BUILD COMMAND ${TBB_SIGNTOOL} $<TARGET_FILE:irml> ${TBB_SIGNTOOL_ARGS})
endif()

install(TARGETS irml
    EXPORT TBBTargets
    LIBRARY
        DESTINATION ${CMAKE_INSTALL_LIBDIR}
        COMPONENT tbb4py
    RUNTIME
        DESTINATION ${CMAKE_INSTALL_BINDIR}
        COMPONENT tbb4py
    ARCHIVE
        DESTINATION ${CMAKE_INSTALL_LIBDIR}
        COMPONENT tbb4py)
