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

option(TBB_VALGRIND_MEMCHECK "Enable scan for memory leaks using Valgrind" OFF)

if (NOT TBB_VALGRIND_MEMCHECK)
    return()
endif()

add_custom_target(memcheck-all
    COMMENT "Run memcheck on all tests")

find_program(VALGRIND_EXE valgrind)

if (NOT VALGRIND_EXE)
    message(FATAL_ERROR "Valgrind executable is not found, add tool to PATH or turn off TBB_VALGRIND_MEMCHECK")
else()
    message(STATUS "Found Valgrind to run memory leak scan")
endif()

file(MAKE_DIRECTORY ${CMAKE_BINARY_DIR}/memcheck)

function(_tbb_run_memcheck test_target)
    set(target_name memcheck-${test_target})
    add_custom_target(${target_name} 
        COMMAND ${VALGRIND_EXE} --leak-check=full --show-leak-kinds=all --log-file=${CMAKE_BINARY_DIR}/memcheck/${target_name}.log -v $<TARGET_FILE:${test_target}>)
    add_dependencies(memcheck-all ${target_name})
endfunction()

add_custom_target(memcheck-short
    COMMENT "Run memcheck scan on specified list")

# List of reasonable and quick enough tests to use in automated memcheck
add_dependencies(memcheck-short 
    memcheck-test_allocators
    memcheck-test_arena_constraints
    memcheck-test_dynamic_link
    memcheck-test_concurrent_lru_cache
    memcheck-conformance_concurrent_unordered_map
    memcheck-conformance_concurrent_unordered_set
    memcheck-conformance_concurrent_map
    memcheck-conformance_concurrent_set
    memcheck-conformance_concurrent_priority_queue
    memcheck-conformance_concurrent_vector
    memcheck-conformance_concurrent_queue
    memcheck-conformance_concurrent_hash_map
    memcheck-test_parallel_for
    memcheck-test_parallel_for_each
    memcheck-test_parallel_reduce
    memcheck-test_parallel_sort
    memcheck-test_parallel_invoke
    memcheck-test_parallel_scan
    memcheck-test_parallel_pipeline
    memcheck-test_eh_algorithms
    memcheck-test_task_group
    memcheck-test_task_arena
    memcheck-test_enumerable_thread_specific
    memcheck-test_resumable_tasks
    memcheck-conformance_mutex
    memcheck-test_function_node
    memcheck-test_multifunction_node
    memcheck-test_broadcast_node
    memcheck-test_buffer_node
    memcheck-test_composite_node
    memcheck-test_continue_node
    memcheck-test_eh_flow_graph
    memcheck-test_flow_graph
    memcheck-test_flow_graph_priorities
    memcheck-test_flow_graph_whitebox
    memcheck-test_indexer_node
    memcheck-test_join_node
    memcheck-test_join_node_key_matching
    memcheck-test_join_node_msg_key_matching
    memcheck-test_priority_queue_node
    memcheck-test_sequencer_node
    memcheck-test_split_node
    memcheck-test_tagged_msg
    memcheck-test_overwrite_node
    memcheck-test_write_once_node
    memcheck-test_async_node
    memcheck-test_input_node
    memcheck-test_profiling
    memcheck-test_concurrent_queue_whitebox
    memcheck-test_intrusive_list
    memcheck-test_semaphore
    memcheck-test_environment_whitebox
    memcheck-test_handle_perror
    memcheck-test_hw_concurrency
    memcheck-test_eh_thread
    memcheck-test_global_control
    memcheck-test_task
    memcheck-test_concurrent_monitor
)
