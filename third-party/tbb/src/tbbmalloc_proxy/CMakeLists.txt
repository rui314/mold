# Copyright (c) 2020-2024 Intel Corporation
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

if (NOT BUILD_SHARED_LIBS)
    return()
endif()

add_library(tbbmalloc_proxy 
    function_replacement.cpp
    proxy.cpp)

if (WIN32)
    target_sources(tbbmalloc_proxy PRIVATE tbbmalloc_proxy.rc)
endif()

add_library(TBB::tbbmalloc_proxy ALIAS tbbmalloc_proxy)

target_compile_definitions(tbbmalloc_proxy
                           PUBLIC
                           $<$<CONFIG:DEBUG>:TBB_USE_DEBUG>
                           PRIVATE
                           __TBBMALLOCPROXY_BUILD)

target_include_directories(tbbmalloc_proxy
    PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/../../include>
    $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>)

if (NOT APPLE AND CMAKE_CXX_COMPILER_ID STREQUAL "GNU")
    # gcc 5.0 and later have -Wno-sized-deallocation options
    set(TBB_WARNING_SUPPRESS ${TBB_WARNING_SUPPRESS}
                             $<$<NOT:$<VERSION_LESS:${CMAKE_CXX_COMPILER_VERSION},5.0>>:-Wno-sized-deallocation>)
endif()

target_compile_options(tbbmalloc_proxy
    PRIVATE
    ${TBB_CXX_STD_FLAG} # TODO: consider making it PUBLIC.
    ${TBB_MMD_FLAG}
    ${TBB_DSE_FLAG}
    ${TBB_WARNING_LEVEL}
    ${TBB_WARNING_SUPPRESS}
    ${TBB_LIB_COMPILE_FLAGS}
    ${TBB_COMMON_COMPILE_FLAGS}
)

set_target_properties(tbbmalloc_proxy PROPERTIES
    VERSION ${TBBMALLOC_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
    SOVERSION ${TBBMALLOC_BINARY_VERSION})

if (UNIX AND NOT APPLE)
    # Avoid use of target_link_libraries here as it changes /DEF option to \DEF on Windows.
    set_target_properties(tbbmalloc_proxy PROPERTIES
        LINK_FLAGS "${TBB_LINK_DEF_FILE_FLAG}\"${CMAKE_CURRENT_SOURCE_DIR}/def/${TBB_DEF_FILE_PREFIX}-proxy.def\""
        LINK_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/def/${TBB_DEF_FILE_PREFIX}-proxy.def"
        DEFINE_SYMBOL "")
endif()

# Prefer using target_link_options instead of target_link_libraries to specify link options because
# target_link_libraries may incorrectly handle some options (on Windows, for example).
if (COMMAND target_link_options)
    target_link_options(tbbmalloc_proxy
        PRIVATE
        ${TBB_LIB_LINK_FLAGS}
        ${TBB_COMMON_LINK_FLAGS}
    )
else()
    target_link_libraries(tbbmalloc_proxy
        PRIVATE
        ${TBB_LIB_LINK_FLAGS}
        ${TBB_COMMON_LINK_FLAGS}
    )
endif()

target_link_libraries(tbbmalloc_proxy
    PRIVATE
    TBB::tbbmalloc
    Threads::Threads
    ${TBB_LIB_LINK_LIBS}
    ${TBB_COMMON_LINK_LIBS}
)

if(TBB_BUILD_APPLE_FRAMEWORKS)
    set_target_properties(tbbmalloc_proxy PROPERTIES 
        FRAMEWORK TRUE
        FRAMEWORK_VERSION ${TBBMALLOC_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
        XCODE_ATTRIBUTE_PRODUCT_BUNDLE_IDENTIFIER com.intel.tbbmalloc-proxy
        MACOSX_FRAMEWORK_IDENTIFIER com.intel.tbbmalloc-proxy
        MACOSX_FRAMEWORK_BUNDLE_VERSION ${TBBMALLOC_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
        MACOSX_FRAMEWORK_SHORT_VERSION_STRING ${TBBMALLOC_BINARY_VERSION})
endif()

tbb_install_target(tbbmalloc_proxy)
