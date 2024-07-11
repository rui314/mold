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

add_library(tbbmalloc 
    backend.cpp
    backref.cpp
    frontend.cpp
    large_objects.cpp
    tbbmalloc.cpp
    ../tbb/itt_notify.cpp)
    
if (WIN32)
    target_sources(tbbmalloc PRIVATE tbbmalloc.rc)
endif()

add_library(TBB::tbbmalloc ALIAS tbbmalloc)

target_compile_definitions(tbbmalloc
                           PUBLIC
                           $<$<CONFIG:DEBUG>:TBB_USE_DEBUG>
                           PRIVATE
                           __TBBMALLOC_BUILD
                           $<$<NOT:$<BOOL:${BUILD_SHARED_LIBS}>>:__TBB_DYNAMIC_LOAD_ENABLED=0>
                           $<$<NOT:$<BOOL:${BUILD_SHARED_LIBS}>>:__TBB_SOURCE_DIRECTLY_INCLUDED=1>)

if (NOT ("${CMAKE_SYSTEM_PROCESSOR}" MATCHES "(armv7-a|aarch64|mips|arm64|riscv)" OR
         "${CMAKE_OSX_ARCHITECTURES}" MATCHES "arm64" OR
         WINDOWS_STORE OR
         TBB_WINDOWS_DRIVER OR
         TBB_SANITIZE MATCHES "thread"))
    target_compile_definitions(tbbmalloc PRIVATE __TBB_USE_ITT_NOTIFY)
endif()

target_include_directories(tbbmalloc
    PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/../../include>
    $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>)

# TODO: fix warnings
if (MSVC)
    # signed unsigned mismatch, declaration hides class member
    set(TBB_WARNING_SUPPRESS ${TBB_WARNING_SUPPRESS} /wd4267 /wd4244 /wd4245 /wd4458)
endif()

target_compile_options(tbbmalloc
    PRIVATE
    ${TBB_CXX_STD_FLAG} # TODO: consider making it PUBLIC.
    ${TBB_MMD_FLAG}
    ${TBB_DSE_FLAG}
    ${TBB_WARNING_LEVEL}
    ${TBB_WARNING_SUPPRESS}
    ${TBB_LIB_COMPILE_FLAGS}
    ${TBBMALLOC_LIB_COMPILE_FLAGS}
    ${TBB_COMMON_COMPILE_FLAGS}
)

enable_language(C)

# Avoid use of target_link_libraries here as it changes /DEF option to \DEF on Windows.
set_target_properties(tbbmalloc PROPERTIES
    DEFINE_SYMBOL ""
    VERSION ${TBBMALLOC_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
    SOVERSION ${TBBMALLOC_BINARY_VERSION}
    LINKER_LANGUAGE C
)

tbb_handle_ipo(tbbmalloc)

if (TBB_DEF_FILE_PREFIX) # If there's no prefix, assume we're using export directives
    set_target_properties(tbbmalloc PROPERTIES
        LINK_FLAGS "${TBB_LINK_DEF_FILE_FLAG}\"${CMAKE_CURRENT_SOURCE_DIR}/def/${TBB_DEF_FILE_PREFIX}-tbbmalloc.def\""
        LINK_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/def/${TBB_DEF_FILE_PREFIX}-tbbmalloc.def"
    )
endif()

set(CMAKE_CXX_IMPLICIT_LINK_LIBRARIES "")

# Prefer using target_link_options instead of target_link_libraries to specify link options because
# target_link_libraries may incorrectly handle some options (on Windows, for example).
if (COMMAND target_link_options)
    target_link_options(tbbmalloc
        PRIVATE
        ${TBB_LIB_LINK_FLAGS}
        ${TBB_COMMON_LINK_FLAGS}
    )
else()
    target_link_libraries(tbbmalloc
        PRIVATE
        ${TBB_LIB_LINK_FLAGS}
        ${TBB_COMMON_LINK_FLAGS}
    )
endif()

target_link_libraries(tbbmalloc
    PRIVATE
    Threads::Threads
    ${TBB_LIB_LINK_LIBS}
    ${TBB_COMMON_LINK_LIBS}
)

if(TBB_BUILD_APPLE_FRAMEWORKS)
    set_target_properties(tbbmalloc PROPERTIES 
        FRAMEWORK TRUE
        FRAMEWORK_VERSION ${TBBMALLOC_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
        XCODE_ATTRIBUTE_PRODUCT_BUNDLE_IDENTIFIER com.intel.tbbmalloc
        MACOSX_FRAMEWORK_IDENTIFIER com.intel.tbbmalloc
        MACOSX_FRAMEWORK_BUNDLE_VERSION ${TBBMALLOC_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
        MACOSX_FRAMEWORK_SHORT_VERSION_STRING ${TBBMALLOC_BINARY_VERSION}
    )
endif()

tbb_install_target(tbbmalloc)
