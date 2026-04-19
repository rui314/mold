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

set(CMAKE_SKIP_BUILD_RPATH TRUE)

function(tbbbind_build TBBBIND_NAME REQUIRED_HWLOC_TARGET)
    if (NOT TARGET ${REQUIRED_HWLOC_TARGET})
        message(STATUS "HWLOC target ${REQUIRED_HWLOC_TARGET} doesn't exist."
                       " The ${TBBBIND_NAME} target cannot be created")
        return()
    endif()
    add_library(${TBBBIND_NAME} tbb_bind.cpp)

    if (WIN32)
        target_sources(${TBBBIND_NAME} PRIVATE tbb_bind.rc)
    endif()

    add_library(TBB::${TBBBIND_NAME} ALIAS ${TBBBIND_NAME})

    target_compile_definitions(${TBBBIND_NAME}
                               PUBLIC
                               $<$<CONFIG:DEBUG>:TBB_USE_DEBUG>
                               PRIVATE
                               __TBBBIND_BUILD)
    target_include_directories(${TBBBIND_NAME}
        PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/../../include>
        $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
        ${HWLOC_INCLUDE_DIRS} # pkg-config defined
    )

    target_compile_options(${TBBBIND_NAME}
        PRIVATE
        ${TBB_CXX_STD_FLAG} # TODO: consider making it PUBLIC.
        ${TBB_MMD_FLAG}
        ${TBB_DSE_FLAG}
        ${TBB_WARNING_LEVEL}
        ${TBB_LIB_COMPILE_FLAGS}
        ${TBB_COMMON_COMPILE_FLAGS}
    )

    # Avoid use of target_link_libraries here as it changes /DEF option to \DEF on Windows.
    set_target_properties(${TBBBIND_NAME} PROPERTIES
        DEFINE_SYMBOL ""
        VERSION ${TBBBIND_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
        SOVERSION ${TBBBIND_BINARY_VERSION}
    )

    tbb_handle_ipo(${TBBBIND_NAME})

    if (TBB_DEF_FILE_PREFIX) # If there's no prefix, assume we're using export directives
        set_target_properties(${TBBBIND_NAME} PROPERTIES
            LINK_FLAGS "${TBB_LINK_DEF_FILE_FLAG}\"${CMAKE_CURRENT_SOURCE_DIR}/def/${TBB_DEF_FILE_PREFIX}-tbbbind.def\""
            LINK_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/def/${TBB_DEF_FILE_PREFIX}-tbbbind.def"
        )
    endif()

    # Prefer using target_link_options instead of target_link_libraries to specify link options because
    # target_link_libraries may incorrectly handle some options (on Windows, for example).
    if (COMMAND target_link_options)
        target_link_options(${TBBBIND_NAME}
            PRIVATE
            ${TBB_LIB_LINK_FLAGS}
            ${TBB_COMMON_LINK_FLAGS}
        )
    else()
        target_link_libraries(${TBBBIND_NAME}
            PRIVATE
            ${TBB_LIB_LINK_FLAGS}
            ${TBB_COMMON_LINK_FLAGS}
        )
    endif()

    target_link_libraries(${TBBBIND_NAME}
        PUBLIC
        ${REQUIRED_HWLOC_TARGET}
        PRIVATE
        ${TBB_LIB_LINK_LIBS}
        ${TBB_COMMON_LINK_LIBS}
    )

    tbb_install_target(${TBBBIND_NAME})

endfunction()

if (NOT DEFINED HWLOC_TARGET_EXPLICITLY_DEFINED AND TARGET PkgConfig::HWLOC)
    message(STATUS "The ${TBBBIND_LIBRARY_NAME} target will be configured using the HWLOC ${HWLOC_VERSION}")
    tbbbind_build(${TBBBIND_LIBRARY_NAME} PkgConfig::HWLOC)
else()
    tbbbind_build(tbbbind     HWLOC::hwloc_1_11)
    tbbbind_build(tbbbind_2_0 HWLOC::hwloc_2   )
    tbbbind_build(tbbbind_2_5 HWLOC::hwloc_2_5 )
endif()

