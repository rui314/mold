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

include(ProcessorCount)

# General function for test target generation
function(tbb_add_test)
    set(oneValueArgs SUBDIR NAME SUFFIX)
    set(multiValueArgs DEPENDENCIES)
    cmake_parse_arguments(_tbb_test "${options}" "${oneValueArgs}" "${multiValueArgs}" ${ARGN})

    set(_tbb_test_TARGET_NAME ${_tbb_test_NAME})
    if (_tbb_test_SUFFIX)
        set(_tbb_test_TARGET_NAME ${_tbb_test_NAME}_${_tbb_test_SUFFIX})
    endif()

    # Define the target for test
    add_executable(${_tbb_test_TARGET_NAME} ${_tbb_test_SUBDIR}/${_tbb_test_NAME}.cpp)
    target_include_directories(${_tbb_test_TARGET_NAME} PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/.. ${CMAKE_CURRENT_SOURCE_DIR})

    target_compile_options(${_tbb_test_TARGET_NAME}
        PRIVATE
        ${TBB_CXX_STD_FLAG}
        ${TBB_WARNING_LEVEL}
        # Warning suppression C4324: structure was padded due to alignment specifier
        $<$<STREQUAL:${MSVC_CXX_ARCHITECTURE_ID},ARM64>:/wd4324>
        ${TBB_TEST_WARNING_FLAGS}
        ${TBB_TEST_COMPILE_FLAGS}
        ${TBB_COMMON_COMPILE_FLAGS}
    )

    if (ANDROID_PLATFORM)
        # Expand the linker rpath by the CMAKE_LIBRARY_OUTPUT_DIRECTORY path since clang compiler from Android SDK
        # doesn't respect the -L flag.
        target_link_libraries(${_tbb_test_TARGET_NAME} PRIVATE "-Wl,--rpath-link,${CMAKE_LIBRARY_OUTPUT_DIRECTORY}")
        add_test(NAME ${_tbb_test_TARGET_NAME}
                 COMMAND ${CMAKE_COMMAND}
                         -DBINARIES_PATH=${CMAKE_LIBRARY_OUTPUT_DIRECTORY}
                         -DTEST_NAME=${_tbb_test_TARGET_NAME}
                         -P ${PROJECT_SOURCE_DIR}/cmake/android/test_launcher.cmake)
    else()
        add_test(NAME ${_tbb_test_TARGET_NAME} COMMAND ${_tbb_test_TARGET_NAME} --force-colors=1 WORKING_DIRECTORY ${TBB_TEST_WORKING_DIRECTORY})
        # Additional testing scenarios if Intel(R) Software Development Emulator is found
        if (UNIX AND ";test_mutex;conformance_mutex;" MATCHES ";${_tbb_test_TARGET_NAME};" AND SDE_EXE)
            add_test(NAME ${_tbb_test_TARGET_NAME}_SDE COMMAND ${SDE_EXE} -nhm -rtm_mode disabled -- ./${_tbb_test_TARGET_NAME} --force-colors=1 WORKING_DIRECTORY ${TBB_TEST_WORKING_DIRECTORY})
            set_property(TEST ${_tbb_test_TARGET_NAME}_SDE PROPERTY ENVIRONMENT ${TBB_TESTS_ENVIRONMENT} APPEND)
        endif()
    endif()

    set_property(TEST ${_tbb_test_TARGET_NAME} PROPERTY ENVIRONMENT ${TBB_TESTS_ENVIRONMENT} APPEND)

    # Prefer using target_link_options instead of target_link_libraries to specify link options because
    # target_link_libraries may incorrectly handle some options (on Windows, for example).
    if (COMMAND target_link_options)
        target_link_options(${_tbb_test_TARGET_NAME} PRIVATE ${TBB_COMMON_LINK_FLAGS} ${TBB_TEST_LINK_FLAGS})
    else()
        target_link_libraries(${_tbb_test_TARGET_NAME} PRIVATE ${TBB_COMMON_LINK_FLAGS} ${TBB_TEST_LINK_FLAGS})
    endif()

    target_compile_definitions(${_tbb_test_TARGET_NAME} PRIVATE
        $<$<CONFIG:DEBUG>:TBB_USE_DEBUG>
        $<$<BOOL:${TBB_CPF}>:__TBB_CPF_BUILD=1>
        $<$<NOT:$<BOOL:${BUILD_SHARED_LIBS}>>:__TBB_DYNAMIC_LOAD_ENABLED=0>
        $<$<NOT:$<BOOL:${BUILD_SHARED_LIBS}>>:__TBB_SOURCE_DIRECTLY_INCLUDED=1>)

    target_link_libraries(${_tbb_test_TARGET_NAME} PRIVATE ${_tbb_test_DEPENDENCIES} Threads::Threads ${TBB_COMMON_LINK_LIBS})

    if (COMMAND _tbb_run_memcheck)
        _tbb_run_memcheck(${_tbb_test_NAME} ${_tbb_test_SUBDIR})
    endif()
endfunction()

# Function for C test target generation
function(tbb_add_c_test)
    set(oneValueArgs SUBDIR NAME)
    set(multiValueArgs DEPENDENCIES)
    cmake_parse_arguments(_tbb_test "${options}" "${oneValueArgs}" "${multiValueArgs}" ${ARGN})

    # Define the target for test

    add_executable(${_tbb_test_NAME} ${_tbb_test_SUBDIR}/${_tbb_test_NAME}.c)
    target_include_directories(${_tbb_test_NAME} PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/.. ${CMAKE_CURRENT_SOURCE_DIR})

    if (ANDROID_PLATFORM)
        add_test(NAME ${_tbb_test_NAME}
                COMMAND ${CMAKE_COMMAND}
                        -DBINARIES_PATH=${CMAKE_LIBRARY_OUTPUT_DIRECTORY}
                        -DTEST_NAME=${_tbb_test_NAME}
                        -P ${PROJECT_SOURCE_DIR}/cmake/android/test_launcher.cmake)
    else()
        add_test(NAME ${_tbb_test_NAME} COMMAND ${_tbb_test_NAME} --force-colors=1 WORKING_DIRECTORY ${TBB_TEST_WORKING_DIRECTORY})
    endif()

    set_property(TEST ${_tbb_test_NAME} PROPERTY ENVIRONMENT ${TBB_TESTS_ENVIRONMENT} APPEND)

    target_compile_definitions(${_tbb_test_NAME} PRIVATE
        $<$<CONFIG:DEBUG>:TBB_USE_DEBUG>
        $<$<BOOL:${TBB_CPF}>:__TBB_CPF_BUILD=1>)

    target_link_libraries(${_tbb_test_NAME} PRIVATE ${_tbb_test_DEPENDENCIES} Threads::Threads)
endfunction()

# Function for lib test target generation
function(tbb_add_lib_test)
    set(oneValueArgs SUBDIR NAME)
    set(multiValueArgs DEPENDENCIES)
    cmake_parse_arguments(_tbb_test "${options}" "${oneValueArgs}" "${multiValueArgs}" ${ARGN})

    add_library(_${_tbb_test_NAME} ${_tbb_test_SUBDIR}/${_tbb_test_NAME}.cpp)

    target_include_directories(_${_tbb_test_NAME}
        PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/../include>
        $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
        PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/..
        ${CMAKE_CURRENT_SOURCE_DIR})

    # TODO: fix warnings
    if (MSVC)
        # signed unsigned mismatch, declaration hides class member
        set(TBB_WARNING_SUPPRESS ${TBB_WARNING_SUPPRESS} /wd4267 /wd4244 /wd4245 /wd4018 /wd4458)
    endif()

    set(TEST_LIB_COMPILE_FLAGS -D_USRDLL)
    # TODO: add ${TBB_WARNING_LEVEL} and fix problems
    target_compile_options(_${_tbb_test_NAME}
        PRIVATE
        ${TBB_CXX_STD_FLAG} # TODO: consider making it PUBLIC.
        ${TBB_MMD_FLAG}
        ${TBB_DSE_FLAG}
        ${TBB_LIB_COMPILE_FLAGS}
        ${TBBMALLOC_LIB_COMPILE_FLAGS}
        ${TBB_COMMON_COMPILE_FLAGS}
        ${TEST_LIB_COMPILE_FLAGS}
    )

    target_compile_definitions(_${_tbb_test_NAME} PRIVATE
        $<$<CONFIG:DEBUG>:TBB_USE_DEBUG>
        $<$<BOOL:${TBB_CPF}>:__TBB_CPF_BUILD=1>
        $<$<NOT:$<BOOL:${BUILD_SHARED_LIBS}>>:__TBB_DYNAMIC_LOAD_ENABLED=0>
        $<$<NOT:$<BOOL:${BUILD_SHARED_LIBS}>>:__TBB_SOURCE_DIRECTLY_INCLUDED=1>)


    # Prefer using target_link_options instead of target_link_libraries to specify link options because
    # target_link_libraries may incorrectly handle some options (on Windows, for example).
    if (COMMAND target_link_options)
        target_link_options(_${_tbb_test_NAME}
            PRIVATE
            ${TBB_LIB_LINK_FLAGS}
            ${TBB_COMMON_LINK_FLAGS}
        )
    else()
        target_link_libraries(_${_tbb_test_NAME}
            PRIVATE
            ${TBB_LIB_LINK_FLAGS}
            ${TBB_COMMON_LINK_FLAGS}
        )
    endif()

    target_link_libraries(_${_tbb_test_NAME}
        PRIVATE
        Threads::Threads
        ${_tbb_test_DEPENDENCIES}
        ${TBB_LIB_LINK_LIBS}
        ${TBB_COMMON_LINK_LIBS}
    )
endfunction()

function(_tbb_get_hwloc_runtime_vars)
    set(oneValueArgs ENV_EXTENSION_VARIABLE)
    set(multiValueArgs HWLOC_VERSION_LIST)
    cmake_parse_arguments(_runtime_vars "${options}" "${oneValueArgs}" "${multiValueArgs}" ${ARGN})

    foreach(HWLOC_VERSION ${_runtime_vars_HWLOC_VERSION_LIST})
        get_target_property(HWLOC_LOCATION HWLOC::${HWLOC_VERSION} IMPORTED_LOCATION)
        get_filename_component(HWLOC_LOCATION_PATH ${HWLOC_LOCATION} DIRECTORY)
        list(APPEND LIBRARIES_PATH ${HWLOC_LOCATION_PATH})
    endforeach()

    if (WIN32)
        string(REPLACE ";" "\;" LIBRARIES_PATH "${LIBRARIES_PATH}\;$ENV{PATH}")
        string(REPLACE "/" "\\" LIBRARIES_PATH "${LIBRARIES_PATH}")
        set(${_runtime_vars_ENV_EXTENSION_VARIABLE} "PATH=${LIBRARIES_PATH}" PARENT_SCOPE)
    else()
        string(REPLACE ";" ":" LIBRARIES_PATH "${LIBRARIES_PATH}:$ENV{LD_LIBRARY_PATH}")
        set(${_runtime_vars_ENV_EXTENSION_VARIABLE} "LD_LIBRARY_PATH=${LIBRARIES_PATH}" PARENT_SCOPE)
    endif()
endfunction()

function(tbb_configure_hwloc_dependent_test)
    set(oneValueArgs SUBDIR NAME SUFFIX TBBBIND_VERSION)
    set(multiValueArgs HWLOC_REQUIRED_VERSION_LIST)
    cmake_parse_arguments(_hwloc_test "${options}" "${oneValueArgs}" "${multiValueArgs}" ${ARGN})

    set(HWLOC_REQUIREMENTS_SATISFIED TRUE)
    foreach(HWLOC_TARGET ${_hwloc_test_HWLOC_REQUIRED_VERSION_LIST})
        if (NOT TARGET HWLOC::${HWLOC_TARGET})
            set(HWLOC_REQUIREMENTS_SATISFIED FALSE)
        endif()
    endforeach()
    if (NOT HWLOC_REQUIREMENTS_SATISFIED)
        return()
    endif()

    list(GET _hwloc_test_HWLOC_REQUIRED_VERSION_LIST 0 TEST_HWLOC_VERSION)
    tbb_add_test(
        SUBDIR ${_hwloc_test_SUBDIR}
        NAME ${_hwloc_test_NAME}
        SUFFIX ${_hwloc_test_SUFFIX}
        DEPENDENCIES TBB::tbb HWLOC::${TEST_HWLOC_VERSION}
    )

    _tbb_get_hwloc_runtime_vars(
        ENV_EXTENSION_VARIABLE HWLOC_RUNTIME_VARS
        HWLOC_VERSION_LIST ${_hwloc_test_HWLOC_REQUIRED_VERSION_LIST}
    )

    set_property(TEST ${_hwloc_test_NAME}_${_hwloc_test_SUFFIX} PROPERTY ENVIRONMENT "${HWLOC_RUNTIME_VARS}" TBB_VERSION=1 APPEND)
    set_tests_properties(${_hwloc_test_NAME}_${_hwloc_test_SUFFIX} PROPERTIES
        PASS_REGULAR_EXPRESSION "oneTBB: TBBBIND.*${_hwloc_test_TBBBIND_VERSION}"
        FAIL_REGULAR_EXPRESSION "Status:.*FAILURE"
    )

    # The tbbbind isn't loading on 32-bit Windows systems with more then 32 available hardware threads
    if (WIN32 AND CMAKE_SIZEOF_VOID_P EQUAL 4 AND SYSTEM_CONCURRENCY GREATER 32)
        set_tests_properties(${_hwloc_test_NAME}_${_hwloc_test_SUFFIX} PROPERTIES
            PASS_REGULAR_EXPRESSION "oneTBB: TBBBIND.*UNAVAILABLE"
            FAIL_REGULAR_EXPRESSION "Status:.*FAILURE"
        )
    else()
        target_compile_definitions(${_hwloc_test_NAME}_${_hwloc_test_SUFFIX} PRIVATE __TBB_HWLOC_VALID_ENVIRONMENT)
    endif()

    add_dependencies(test_suite_arena_constraints ${_hwloc_test_NAME}_${_hwloc_test_SUFFIX})
endfunction()

function(tbb_add_tbbbind_test)
    set(oneValueArgs SUBDIR NAME)
    cmake_parse_arguments(_tbbbind_test "${options}" "${oneValueArgs}" "${multiValueArgs}" ${ARGN})

    tbb_add_test(SUBDIR ${_tbbbind_test_SUBDIR} NAME ${_tbbbind_test_NAME} DEPENDENCIES TBB::tbb)
    add_dependencies(test_suite_arena_constraints ${_tbbbind_test_NAME})

    set_property(TEST ${_tbbbind_test_NAME} PROPERTY ENVIRONMENT TBB_VERSION=1 APPEND)

    # Handle the case when HWLOC was found using pkg-config
    if (NOT DEFINED HWLOC_TARGET_EXPLICITLY_DEFINED AND TARGET PkgConfig::HWLOC)
        set_tests_properties(${_tbbbind_test_NAME} PROPERTIES
            PASS_REGULAR_EXPRESSION "oneTBB: TBBBIND.*${TBBBIND_LIBRARY_NAME}"
            FAIL_REGULAR_EXPRESSION "Status: FAILURE!"
        )
        target_link_libraries(${_tbbbind_test_NAME} PRIVATE PkgConfig::HWLOC)
        target_compile_definitions(${_tbbbind_test_NAME} PRIVATE __TBB_HWLOC_VALID_ENVIRONMENT)
        return()
    endif()

    # Disable all HWLOC dependent tests in case of unsupported environment.
    if (TBB_WINDOWS_DRIVER OR ANDROID_PLATFORM OR APPLE OR NOT BUILD_SHARED_LIBS)
        return()
    endif()
    ProcessorCount(SYSTEM_CONCURRENCY)

    set_tests_properties(${_tbbbind_test_NAME} PROPERTIES
        PASS_REGULAR_EXPRESSION "oneTBB: TBBBIND.*UNAVAILABLE"
        FAIL_REGULAR_EXPRESSION "Status:.*FAILURE"
    )

    if (TARGET HWLOC::hwloc_2_5 AND NOT HWLOC_2_5_TESTS_STATUS_SHOWN)
        message(STATUS "HWLOC 2.5 dependent tests were enabled.")
        set(HWLOC_2_5_TESTS_STATUS_SHOWN TRUE PARENT_SCOPE)
    endif()

    if (TARGET HWLOC::hwloc_2 AND NOT HWLOC_2_TESTS_STATUS_SHOWN)
        message(STATUS "HWLOC 2 dependent tests were enabled.")
        set(HWLOC_2_TESTS_STATUS_SHOWN TRUE PARENT_SCOPE)
    endif()

    if (TARGET HWLOC::hwloc_1_11 AND NOT HWLOC_1_11_TESTS_STATUS_SHOWN)
        message(STATUS "HWLOC 1.11 dependent tests were enabled.")
        set(HWLOC_1_11_TESTS_STATUS_SHOWN TRUE PARENT_SCOPE)
    endif()

    list(APPEND HWLOC_TEST_CASES
        hwloc_2_5
        hwloc_2
        hwloc_1_11
        hwloc_2_5_hwloc_2
        hwloc_2_5_hwloc_1_11
        hwloc_2_hwloc_1_11
        hwloc_2_5_hwloc_2_hwloc_1_11
        incompatible_hwlocs_1_11_vs_2_5
        incompatible_hwlocs_1_11_vs_2
    )

    list(APPEND HWLOC_TEST_CASE_0_VARS tbbbind_2_5 "hwloc_2_5")
    list(APPEND HWLOC_TEST_CASE_1_VARS tbbbind_2   "hwloc_2")
    list(APPEND HWLOC_TEST_CASE_2_VARS tbbbind     "hwloc_1_11")
    list(APPEND HWLOC_TEST_CASE_3_VARS tbbbind_2_5 "hwloc_2_5,hwloc_2")
    list(APPEND HWLOC_TEST_CASE_4_VARS tbbbind_2_5 "hwloc_2_5,hwloc_1_11")
    list(APPEND HWLOC_TEST_CASE_5_VARS tbbbind_2   "hwloc_2,hwloc_1_11")
    list(APPEND HWLOC_TEST_CASE_6_VARS tbbbind_2_5 "hwloc_2_5,hwloc_2,hwloc_1_11")
    list(APPEND HWLOC_TEST_CASE_7_VARS tbbbind_2_5 "hwloc_1_11,hwloc_2_5")
    list(APPEND HWLOC_TEST_CASE_8_VARS tbbbind_2   "hwloc_1_11,hwloc_2")

    foreach(TEST_CASE ${HWLOC_TEST_CASES})
        list(FIND HWLOC_TEST_CASES ${TEST_CASE} TEST_CASE_INDEX)
        list(GET HWLOC_TEST_CASE_${TEST_CASE_INDEX}_VARS 0 TEST_CASE_TBBBIND_EXPECTED_VERSION)
        list(GET HWLOC_TEST_CASE_${TEST_CASE_INDEX}_VARS 1 TEST_CASE_TBBBIND_HWLOC_REQUIRED_VERSIONS)
        string(REPLACE "," ";" TEST_CASE_TBBBIND_HWLOC_REQUIRED_VERSIONS "${TEST_CASE_TBBBIND_HWLOC_REQUIRED_VERSIONS}")

        tbb_configure_hwloc_dependent_test(
            SUBDIR ${_tbbbind_test_SUBDIR}
            NAME ${_tbbbind_test_NAME}
            SUFFIX ${TEST_CASE}
            TBBBIND_VERSION ${TEST_CASE_TBBBIND_EXPECTED_VERSION}
            HWLOC_REQUIRED_VERSION_LIST ${TEST_CASE_TBBBIND_HWLOC_REQUIRED_VERSIONS}
        )
    endforeach()
endfunction()

# Copy libraries to test folder to make it visible during tests execution if external TBB is tested.
# TODO: check and update for multi-config generators.
if (TBB_FOUND)
    list(APPEND _tbb_test_components tbb tbbmalloc tbbmalloc_proxy tbbbind tbbbind_2_0 tbbbind_2_5)
    foreach(_component ${_tbb_test_components})
        if (TARGET TBB::${_component})
            get_property(${_component}_lib_file_location TARGET TBB::${_component} PROPERTY LOCATION)
            file(COPY ${${_component}_lib_file_location} DESTINATION ${CMAKE_RUNTIME_OUTPUT_DIRECTORY})
            unset(${_component}_lib_file_location CACHE)
        endif()
    endforeach()
    unset(_tbb_test_components)
endif()

# Find Intel(R) Software Development Emulator to run test_mutex and conformance_mutex for coverage
set(_sde_find_name sde)

if (UNIX AND TBB_ARCH EQUAL 64)
    set(_sde_find_name sde64)
endif()

find_program(SDE_EXE
    NAMES ${_sde_find_name}
    PATHS ENV PATH
    PATH_SUFFIXES bin)

unset(_sde_find_name)

# Common target for the tbbbind related tests
add_custom_target(test_suite_arena_constraints)

# Check support for --no-as-needed linker option
if (MINGW OR NOT WIN32)
    include(CheckCXXSourceCompiles)
    set(CMAKE_REQUIRED_LIBRARIES "-Wl,--no-as-needed")
    check_cxx_source_compiles("int main(int, char*[]) { return 0; }" LINKER_HAS_NO_AS_NEEDED)
    unset(CMAKE_REQUIRED_LIBRARIES)
endif()

if (TARGET TBB::tbb)
    # Define the tests
    tbb_add_test(SUBDIR tbb NAME test_tick_count DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_allocators DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_arena_priorities DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_dynamic_link DEPENDENCIES TBB::tbb)
    if (LINKER_HAS_NO_AS_NEEDED)
        # The linker may not detect a dependency on pthread in static variable constructors.
        target_link_libraries(test_dynamic_link PRIVATE "-Wl,--no-as-needed")
    endif()
    if (APPLE OR ANDROID_PLATFORM)
        target_link_libraries(test_dynamic_link PRIVATE -rdynamic)
    endif()
    if (WIN32)
        tbb_add_test(SUBDIR tbb NAME test_numa_dist DEPENDENCIES TBB::tbb)
    endif()
    tbb_add_test(SUBDIR tbb NAME test_collaborative_call_once DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_lru_cache DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_unordered_map DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_unordered_set DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_map DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_set DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_priority_queue DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_partitioner DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_parallel_for DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_parallel_for_each DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_parallel_reduce DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_parallel_sort DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_parallel_invoke DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_parallel_scan DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_parallel_pipeline DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_eh_algorithms DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_blocked_range DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_vector DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_task_group DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_hash_map DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_task_arena DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_enumerable_thread_specific DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_queue DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_resumable_tasks DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_mutex DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_function_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_multifunction_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_broadcast_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_buffer_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_composite_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_continue_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_eh_flow_graph DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_flow_graph DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_flow_graph_priorities DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_flow_graph_whitebox DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_indexer_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_join_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_join_node_key_matching DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_join_node_key_matching_n_args DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_join_node_msg_key_matching DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_join_node_msg_key_matching_n_args DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_join_node_preview DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_limiter_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_priority_queue_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_queue_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_sequencer_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_split_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_tagged_msg DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_overwrite_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_write_once_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_async_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_input_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_profiling DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_concurrent_queue_whitebox DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_intrusive_list DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_semaphore DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_environment_whitebox DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_hw_concurrency DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_eh_thread DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_global_control DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_task DEPENDENCIES TBB::tbb)

    if (TBB_FUZZ_TESTING AND NOT WIN32)
        if (NOT ((CMAKE_CXX_COMPILER_ID STREQUAL Clang) OR (CMAKE_CXX_COMPILER_ID STREQUAL IntelLLVM)))
            message(FATAL_ERROR "Fuzzing requires Clang or IntelLLVM compiler.")
        endif()
        tbb_add_test(SUBDIR tbb NAME test_fuzzing)
        add_dependencies(test_fuzzing test_task)
        target_compile_definitions(test_fuzzing PRIVATE CMD="$<TARGET_FILE:test_task> >/dev/null 2>&1")
        target_link_options(test_fuzzing PRIVATE -fsanitize=fuzzer)
    endif()

    tbb_add_test(SUBDIR tbb NAME test_concurrent_monitor DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR tbb NAME test_scheduler_mix DEPENDENCIES TBB::tbb)

    # test_handle_perror
    tbb_add_test(SUBDIR tbb NAME test_handle_perror)
    target_include_directories(test_handle_perror PRIVATE
        $<TARGET_PROPERTY:TBB::tbb,INTERFACE_INCLUDE_DIRECTORIES>
    )

    # HWLOC related test
    if (NOT TBB_EMSCRIPTEN)
        tbb_add_tbbbind_test(SUBDIR tbb NAME test_arena_constraints)
    endif()

    if ((NOT "${CMAKE_SYSTEM_PROCESSOR}" MATCHES "mips") AND (NOT TBB_EMSCRIPTEN))
        # TODO: Fix for MIPS
        tbb_add_test(SUBDIR tbb NAME test_tbb_fork DEPENDENCIES TBB::tbb)
    endif()

    tbb_add_test(SUBDIR tbb NAME test_tbb_header DEPENDENCIES TBB::tbb)
    target_sources(test_tbb_header PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/tbb/test_tbb_header_secondary.cpp)
    if (TBB_OPENMP_FLAG AND NOT CMAKE_SYSTEM_PROCESSOR MATCHES "(mips)")
        tbb_add_test(SUBDIR tbb NAME test_openmp DEPENDENCIES TBB::tbb)

        set_target_properties(test_openmp PROPERTIES COMPILE_FLAGS ${TBB_OPENMP_FLAG})

        if (NOT TBB_OPENMP_NO_LINK_FLAG)
            set_target_properties(test_openmp PROPERTIES LINK_FLAGS ${TBB_OPENMP_FLAG})
        endif()
    endif()

    # Define the conformance tests
    tbb_add_test(SUBDIR conformance NAME conformance_tick_count DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_allocators DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_mutex DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_task_group DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_task_group_context DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_task_arena DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_collaborative_call_once DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_concurrent_lru_cache DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_concurrent_unordered_map DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_concurrent_unordered_set DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_concurrent_map DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_concurrent_set DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_concurrent_priority_queue DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_parallel_for DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_parallel_for_each DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_parallel_reduce DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_parallel_scan DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_parallel_sort DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_parallel_pipeline DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_parallel_invoke DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_blocked_range DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_blocked_range2d DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_blocked_range3d DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_blocked_rangeNd DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_concurrent_vector DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_global_control DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_concurrent_hash_map DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_enumerable_thread_specific DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_combinable DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_concurrent_queue DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_resumable_tasks DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_version DEPENDENCIES TBB::tbb)
    # functional nodes conformance
    tbb_add_test(SUBDIR conformance NAME conformance_function_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_multifunction_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_input_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_continue_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_async_node DEPENDENCIES TBB::tbb)
    # buffering nodes conformance
    tbb_add_test(SUBDIR conformance NAME conformance_overwrite_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_write_once_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_buffer_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_queue_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_priority_queue_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_sequencer_node DEPENDENCIES TBB::tbb)
    # service nodes conformance
    tbb_add_test(SUBDIR conformance NAME conformance_limiter_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_broadcast_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_composite_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_indexer_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_split_node DEPENDENCIES TBB::tbb)
    tbb_add_test(SUBDIR conformance NAME conformance_join_node DEPENDENCIES TBB::tbb)
    # flowraph auxiliary conformance
    # TODO: add conformance tests for graph_node, continue_msg, tagged_msg, copy_body, input_port, output_port, make_edge, remove_edge
    tbb_add_test(SUBDIR conformance NAME conformance_graph DEPENDENCIES TBB::tbb)

    # HWLOC related conformance
    if (NOT TBB_EMSCRIPTEN)
        tbb_add_tbbbind_test(SUBDIR conformance NAME conformance_arena_constraints)
    endif()

    if (MSVC AND BUILD_SHARED_LIBS AND CMAKE_VERSION VERSION_GREATER 3.13) # LINK_OPTIONS property first appeared in 3.13
                                                    # version of the CMake
        tbb_add_test(SUBDIR tbb NAME test_implicit_linkage_on_windows)
        # TODO: consider setting environment instead of passing additional
        #       compiler and linker options
        target_include_directories(test_implicit_linkage_on_windows PRIVATE
        $<TARGET_PROPERTY:TBB::tbb,INTERFACE_INCLUDE_DIRECTORIES>)
        set_target_properties(test_implicit_linkage_on_windows PROPERTIES
        LINK_OPTIONS /LIBPATH:$<TARGET_LINKER_FILE_DIR:TBB::tbb>)
        add_dependencies(test_implicit_linkage_on_windows TBB::tbb)
    endif()
endif()

if (TARGET TBB::tbbmalloc)
    # TBB allocator tests
    if (NOT "${CMAKE_SYSTEM_PROCESSOR}" MATCHES "mips")
        if (NOT TBB_EMSCRIPTEN)
            # Define TBB malloc tests
	    tbb_add_test(SUBDIR tbbmalloc NAME test_scalable_allocator DEPENDENCIES TBB::tbbmalloc)
            tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_pools DEPENDENCIES TBB::tbbmalloc)
            tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_init_shutdown DEPENDENCIES TBB::tbbmalloc)
            tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_regression DEPENDENCIES TBB::tbbmalloc)
            if (TARGET TBB::tbb)
                tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_shutdown_hang DEPENDENCIES TBB::tbb TBB::tbbmalloc)
            endif()

            if (NOT (WINDOWS_STORE OR TBB_WINDOWS_DRIVER))
                # TODO: Consider adding following tests on WINDOWS_STORE and TBB_WINDOWS_DRIVER platforms
                tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_compliance DEPENDENCIES TBB::tbbmalloc)
                tbb_add_lib_test(SUBDIR tbbmalloc NAME test_malloc_used_by_lib DEPENDENCIES TBB::tbbmalloc)
                tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_used_by_lib DEPENDENCIES _test_malloc_used_by_lib)
                tbb_add_lib_test(SUBDIR tbbmalloc NAME test_malloc_lib_unload)
                tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_lib_unload DEPENDENCIES _test_malloc_lib_unload)
            endif()

            enable_language(C)
            tbb_add_c_test(SUBDIR tbbmalloc NAME test_malloc_pure_c DEPENDENCIES TBB::tbbmalloc)
        endif()
        # ----------------------------------------------------------------------------------------
        # Whitebox testing

        add_executable(test_malloc_whitebox tbbmalloc/test_malloc_whitebox.cpp)

        target_include_directories(test_malloc_whitebox
            PRIVATE
            ${CMAKE_CURRENT_SOURCE_DIR}/../include
            ${CMAKE_CURRENT_SOURCE_DIR}/..
            ${CMAKE_CURRENT_SOURCE_DIR})
        target_compile_definitions(test_malloc_whitebox PRIVATE __TBBMALLOC_BUILD)
        target_compile_options(test_malloc_whitebox
            PRIVATE
            ${TBB_CXX_STD_FLAG}
            ${TBB_WARNING_SUPPRESS}
            ${TBB_TEST_COMPILE_FLAGS}
            ${TBB_COMMON_COMPILE_FLAGS}
            ${TBBMALLOC_LIB_COMPILE_FLAGS}
        )
        if (ANDROID_PLATFORM)
            add_test(NAME test_malloc_whitebox
                    COMMAND ${CMAKE_COMMAND}
                            -DBINARIES_PATH=${CMAKE_LIBRARY_OUTPUT_DIRECTORY}
                            -DTEST_NAME=test_malloc_whitebox
                            -P ${PROJECT_SOURCE_DIR}/cmake/android/test_launcher.cmake)
        else()
            add_test(NAME test_malloc_whitebox COMMAND test_malloc_whitebox --force-colors=1)
        endif()
        if (COMMAND target_link_options)
            target_link_options(test_malloc_whitebox PRIVATE ${TBB_COMMON_LINK_FLAGS})
        else()
            target_link_libraries(test_malloc_whitebox PRIVATE ${TBB_COMMON_LINK_FLAGS})
        endif()
        target_link_libraries(test_malloc_whitebox PRIVATE Threads::Threads ${TBB_COMMON_LINK_LIBS})

        # ------------------------------------------------------------------------------------------

        # Define TBB malloc conformance tests
        # tbbmalloc_add_test(conformance conformance_scalable_allocator)

        if ("${CMAKE_MSVC_RUNTIME_LIBRARY}" STREQUAL MultiThreaded OR "${CMAKE_MSVC_RUNTIME_LIBRARY}" STREQUAL MultiThreadedDebug)
            if ("${CMAKE_MSVC_RUNTIME_LIBRARY}" STREQUAL MultiThreaded)
                set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreadedDLL)
            else()
                set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreadedDebugDLL)
            endif()
        endif()

        # Thread Sanitizer overloads memory management routines that conflicts with tbbmalloc_proxy.
        if (BUILD_SHARED_LIBS AND NOT TBB_SANITIZE MATCHES "thread" AND TBBMALLOC_PROXY_BUILD AND NOT MSVC_CXX_ARCHITECTURE_ID MATCHES "ARM64")
            # Define TBB malloc proxy tests
            tbb_add_lib_test(SUBDIR tbbmalloc NAME test_malloc_atexit DEPENDENCIES TBB::tbbmalloc_proxy TBB::tbbmalloc)
	    if (NOT TBB_EMSCRIPTEN)
	        tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_atexit DEPENDENCIES TBB::tbbmalloc_proxy TBB::tbbmalloc _test_malloc_atexit)
	    endif()
            tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_overload DEPENDENCIES TBB::tbbmalloc_proxy)
            tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_overload_disable DEPENDENCIES TBB::tbbmalloc_proxy TBB::tbbmalloc) # safer_msize call need to be available
            tbb_add_test(SUBDIR tbbmalloc NAME test_malloc_new_handler DEPENDENCIES TBB::tbbmalloc_proxy)
        endif()
    endif()
endif()

unset(HWLOC_2_5_TESTS_STATUS_SHOWN)
unset(HWLOC_2_TESTS_STATUS_SHOWN)
unset(HWLOC_1_11_TESTS_STATUS_SHOWN)
