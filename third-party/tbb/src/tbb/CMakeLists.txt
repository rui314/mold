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

add_library(tbb
    address_waiter.cpp
    allocator.cpp
    arena.cpp
    arena_slot.cpp
    concurrent_bounded_queue.cpp
    dynamic_link.cpp
    exception.cpp
    governor.cpp
    global_control.cpp
    itt_notify.cpp
    main.cpp
    market.cpp
    tcm_adaptor.cpp
    misc.cpp
    misc_ex.cpp
    observer_proxy.cpp
    parallel_pipeline.cpp
    private_server.cpp
    profiling.cpp
    rml_tbb.cpp
    rtm_mutex.cpp
    rtm_rw_mutex.cpp
    semaphore.cpp
    small_object_pool.cpp
    task.cpp
    task_dispatcher.cpp
    task_group_context.cpp
    thread_dispatcher.cpp
    thread_request_serializer.cpp
    threading_control.cpp
    version.cpp
    queuing_rw_mutex.cpp)

add_library(TBB::tbb ALIAS tbb)

if (WIN32)
    target_sources(tbb PRIVATE tbb.rc)
    set_target_properties(tbb PROPERTIES OUTPUT_NAME "tbb${TBB_BINARY_VERSION}")
endif()

# TODO: Add statistics.cpp

target_compile_definitions(tbb
                           PUBLIC
                           $<$<CONFIG:DEBUG>:TBB_USE_DEBUG>
                           PRIVATE
                           __TBB_BUILD
                           ${TBB_RESUMABLE_TASKS_USE_THREADS}
                           $<$<NOT:$<BOOL:${BUILD_SHARED_LIBS}>>:__TBB_DYNAMIC_LOAD_ENABLED=0>
                           $<$<NOT:$<BOOL:${BUILD_SHARED_LIBS}>>:__TBB_SOURCE_DIRECTLY_INCLUDED=1>)

if (NOT ("${CMAKE_SYSTEM_PROCESSOR}" MATCHES "(armv7-a|aarch64|mips|arm64|riscv)" OR
         "${CMAKE_OSX_ARCHITECTURES}" MATCHES "arm64" OR
         WINDOWS_STORE OR
         TBB_WINDOWS_DRIVER))
    target_compile_definitions(tbb PRIVATE __TBB_USE_ITT_NOTIFY)
endif()

target_include_directories(tbb
    PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/../../include>
    $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>)

target_compile_options(tbb
    PRIVATE
    ${TBB_CXX_STD_FLAG} # TODO: consider making it PUBLIC.
    ${TBB_MMD_FLAG}
    ${TBB_DSE_FLAG}
    ${TBB_WARNING_LEVEL}
    ${TBB_WARNING_SUPPRESS}
    ${TBB_LIB_COMPILE_FLAGS}
    ${TBB_COMMON_COMPILE_FLAGS}
)

# Avoid use of target_link_libraries here as it changes /DEF option to \DEF on Windows.
set_target_properties(tbb PROPERTIES
    DEFINE_SYMBOL ""
    VERSION ${TBB_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
    SOVERSION ${TBB_BINARY_VERSION}
)

tbb_handle_ipo(tbb)

if (TBB_DEF_FILE_PREFIX) # If there's no prefix, assume we're using export directives
    set_target_properties(tbb PROPERTIES
        LINK_FLAGS "${TBB_LINK_DEF_FILE_FLAG}\"${CMAKE_CURRENT_SOURCE_DIR}/def/${TBB_DEF_FILE_PREFIX}-tbb.def\""
        LINK_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/def/${TBB_DEF_FILE_PREFIX}-tbb.def"
    )
endif()

# Prefer using target_link_options instead of target_link_libraries to specify link options because
# target_link_libraries may incorrectly handle some options (on Windows, for example).
if (COMMAND target_link_options)
    target_link_options(tbb
        PRIVATE
        ${TBB_LIB_LINK_FLAGS}
        ${TBB_COMMON_LINK_FLAGS}
    )
else()
    target_link_libraries(tbb
        PRIVATE
        ${TBB_LIB_LINK_FLAGS}
        ${TBB_COMMON_LINK_FLAGS}
    )
endif()

target_link_libraries(tbb
    PRIVATE
    Threads::Threads
    ${TBB_LIB_LINK_LIBS}
    ${TBB_COMMON_LINK_LIBS}
)

if(TBB_BUILD_APPLE_FRAMEWORKS)
    set_target_properties(tbb PROPERTIES
        FRAMEWORK TRUE
        FRAMEWORK_VERSION ${TBB_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
        XCODE_ATTRIBUTE_PRODUCT_BUNDLE_IDENTIFIER com.intel.tbb
        MACOSX_FRAMEWORK_IDENTIFIER com.intel.tbb
        MACOSX_FRAMEWORK_BUNDLE_VERSION ${TBB_BINARY_VERSION}.${TBB_BINARY_MINOR_VERSION}
        MACOSX_FRAMEWORK_SHORT_VERSION_STRING ${TBB_BINARY_VERSION})
endif()

tbb_install_target(tbb)

if (TBB_INSTALL)
    if (MSVC)
        # Create a copy of target linker file (tbb<ver>[_debug].lib) with legacy name (tbb[_debug].lib)
        # to support previous user experience for linkage.
        install(FILES
                $<TARGET_LINKER_FILE:tbb>
                DESTINATION lib
                CONFIGURATIONS RelWithDebInfo Release MinSizeRel
                RENAME tbb.lib
                COMPONENT devel
        )

        install(FILES
                $<TARGET_LINKER_FILE:tbb>
                DESTINATION lib
                CONFIGURATIONS Debug
                RENAME tbb_debug.lib
                COMPONENT devel
        )
    endif()

    set(_tbb_pc_lib_name tbb)

    if (WIN32)
        set(_tbb_pc_lib_name ${_tbb_pc_lib_name}${TBB_BINARY_VERSION})
    endif()

    if (CMAKE_SIZEOF_VOID_P EQUAL 8)
        set(TBB_PC_NAME tbb)
    else()
        set(TBB_PC_NAME tbb32)
    endif()

    set(_prefix_for_pc_file "${CMAKE_INSTALL_PREFIX}")

    if (IS_ABSOLUTE "${CMAKE_INSTALL_LIBDIR}")
        set(_libdir_for_pc_file "${CMAKE_INSTALL_LIBDIR}")
    else()
        set(_libdir_for_pc_file "\${prefix}/${CMAKE_INSTALL_LIBDIR}")
    endif()

    if (IS_ABSOLUTE "${CMAKE_INSTALL_INCLUDEDIR}")
        set(_includedir_for_pc_file "${CMAKE_INSTALL_INCLUDEDIR}")
    else()
        set(_includedir_for_pc_file "\${prefix}/${CMAKE_INSTALL_INCLUDEDIR}")
    endif()

    configure_file(${PROJECT_SOURCE_DIR}/integration/pkg-config/tbb.pc.in ${CMAKE_CURRENT_BINARY_DIR}/${TBB_PC_NAME}.pc @ONLY)
    install(FILES ${CMAKE_CURRENT_BINARY_DIR}/${TBB_PC_NAME}.pc
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/pkgconfig/
            COMPONENT devel)
endif()

if (COMMAND tbb_gen_vars)
    tbb_gen_vars(tbb)
endif()
