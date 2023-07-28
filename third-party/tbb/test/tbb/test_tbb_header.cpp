/*
    Copyright (c) 2005-2021 Intel Corporation

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

//! \file test_tbb_header.cpp
//! \brief Test for [all] specification

/**
    This test ensures that tbb.h brings in all the public TBB interface definitions,
    and if all the necessary symbols are exported from the library.

    Most of the checks happen at the compilation or link phases.
**/

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#ifndef NOMINMAX
#define NOMINMAX
#endif // NOMINMAX

#define __TBB_NO_IMPLICIT_LINKAGE 1


#if __TBB_TEST_SECONDARY
    // Test _DEBUG macro custom definitions.
    #if TBB_USE_DEBUG
        #ifdef _DEBUG
            #undef _DEBUG
        #endif /* _DEBUG */
        // Check that empty value successfully enables the debug mode.
        #define _DEBUG
        static bool isDebugExpected = true;
    #else
        // Check that zero value does not enable the debug mode.
        #define _DEBUG 0x0
        static bool isDebugExpected = false;
    #endif /* TBB_USE_DEBUG */
    #define DO_TEST_DEBUG_MACRO 1
#else
    // Test default definitions of _DEBUG.
    #if _DEBUG
        static bool isDebugExpected = true;
        #define DO_TEST_DEBUG_MACRO 1
    #elif _MSC_VER
        // for MSVC, _DEBUG not defined indicates a release mode.
        static bool isDebugExpected = false;
        #define DO_TEST_DEBUG_MACRO 1
    #endif /* _DEBUG */
#endif /* __TBB_TEST_SECONDARY */

#if DO_TEST_DEBUG_MACRO
// Reset TBB_USE_DEBUG defined in makefiles.
#undef TBB_USE_DEBUG
#endif /* DO_TEST_DEBUG_MACRO */
#define __TBB_CONFIG_PREPROC_ONLY _MSC_VER // For MSVC, prevent including standard headers in tbb_config.h
#include "common/config.h"

#include "oneapi/tbb/detail/_config.h"

#if !TBB_USE_DEBUG && defined(_DEBUG)
// TBB_USE_DEBUG is 0 but _DEBUG is defined, it means that _DEBUG is 0
// MSVC C++ headers consider any definition of _DEBUG, including 0, as debug mode
#undef _DEBUG
#endif /* !TBB_USE_DEBUG && defined(_DEBUG) */

#include "tbb/tbb.h"

#if !__TBB_TEST_SECONDARY
#include "common/test.h"
#endif

static volatile size_t g_sink;

#define TestTypeDefinitionPresence( Type ) g_sink = sizeof(tbb::Type);
#define TestTypeDefinitionPresence2(TypeStart, TypeEnd) g_sink = sizeof(tbb::TypeStart,TypeEnd);
#define TestTypeDefinitionPresence3(TypeStart, TypeMid, TypeEnd) g_sink = sizeof(tbb::TypeStart,TypeMid,TypeEnd);
#define TestFuncDefinitionPresence(Fn, Args, ReturnType) { ReturnType (*pfn)Args = &tbb::Fn; (void)pfn; }

struct Body {
    void operator() () const {}
};
struct Body1 {
    void operator() ( int ) const {}
};
struct Body1a { // feeder body for parallel_do
    void operator() ( int, tbb::feeder<int>& ) const {}
};
struct Body1b { // binary operator for reduction
    int operator() ( const int, const int ) const { return 0; }
};
struct Body1bc { // binary operator for comparison
    bool operator() (const int, const int) const { return false; }
};
struct Body2 {
    Body2 () {}
    Body2 ( const Body2&, tbb::split ) {}
    void operator() ( const tbb::blocked_range<int>& ) const {}
    void join( const Body2& ) {}
};
struct Body2a { // for lambda-friendly parallel_reduce
    int operator() ( const tbb::blocked_range<int>&, const int ) const { return 0; }
};
struct Body3 { // for parallel_scan
    Body3 () {}
    Body3 ( const Body3&, tbb::split ) {}
    void operator() ( const tbb::blocked_range2d<int>&, tbb::pre_scan_tag ) const {}
    void operator() ( const tbb::blocked_range2d<int>&, tbb::final_scan_tag ) const {}
    void reverse_join( Body3& ) {}
    void assign( const Body3& ) {}
};
struct Body3a { // for lambda-friednly parallel_scan
    int operator() ( const tbb::blocked_range<int>&, const int, bool ) const { return 0; }
};
struct Msg {};

// Test if all the necessary symbols are exported for the exceptions thrown by TBB.
// Missing exports result either in link error or in runtime assertion failure.
#include <stdexcept>

template <typename E>
void TestExceptionClassExports ( const E& exc, tbb::detail::exception_id eid ) {
    CHECK( eid < tbb::detail::exception_id::last_entry );
#if TBB_USE_EXCEPTIONS
    for ( int i = 0; i < 2; ++i ) {
        try {
            if ( i == 0 )
                throw exc;
            else
                tbb::detail::throw_exception( eid );
        }
        catch ( E& e ) {
            CHECK_MESSAGE( e.what(), "Missing what() string" );
        }
        catch ( ... ) {
            CHECK_MESSAGE( false, "Unrecognized exception. Likely RTTI related exports are missing" );
        }
    }
#else /* TBB_USE_EXCEPTIONS */
    (void)exc;
#endif /* TBB_USE_EXCEPTIONS */
}

static void TestExceptionClassesExports () {
    TestExceptionClassExports( std::bad_alloc(), tbb::detail::exception_id::bad_alloc );
    TestExceptionClassExports( tbb::bad_last_alloc(), tbb::detail::exception_id::bad_last_alloc );
    TestExceptionClassExports( std::invalid_argument("test"), tbb::detail::exception_id::nonpositive_step );
    TestExceptionClassExports( std::out_of_range("test"), tbb::detail::exception_id::out_of_range );
    TestExceptionClassExports( tbb::missing_wait(), tbb::detail::exception_id::missing_wait );
    TestExceptionClassExports( std::out_of_range("test"), tbb::detail::exception_id::invalid_load_factor );
    TestExceptionClassExports( std::length_error("test"), tbb::detail::exception_id::reservation_length_error );
    TestExceptionClassExports( std::out_of_range("test"), tbb::detail::exception_id::invalid_key );
    TestExceptionClassExports( tbb::user_abort(), tbb::detail::exception_id::user_abort );
    TestExceptionClassExports( std::runtime_error("test"), tbb::detail::exception_id::bad_tagged_msg_cast );
}

#if __TBB_CPF_BUILD
// These names are only tested in "preview" configuration
// When a feature becomes fully supported, its names should be moved to the main test
static void TestPreviewNames() {
    TestTypeDefinitionPresence2( blocked_rangeNd<int,4> );
    TestTypeDefinitionPresence2( concurrent_lru_cache<int, int> );
    TestTypeDefinitionPresence( isolated_task_group );
}
#endif

static void DefinitionPresence() {
    TestTypeDefinitionPresence( cache_aligned_allocator<int> );
    TestTypeDefinitionPresence( tbb_hash_compare<int> );
    TestTypeDefinitionPresence2( concurrent_hash_map<int, int> );
    TestTypeDefinitionPresence2( concurrent_unordered_map<int, int> );
    TestTypeDefinitionPresence2( concurrent_unordered_multimap<int, int> );
    TestTypeDefinitionPresence( concurrent_unordered_set<int> );
    TestTypeDefinitionPresence( concurrent_unordered_multiset<int> );
    TestTypeDefinitionPresence2( concurrent_map<int, int> );
    TestTypeDefinitionPresence2( concurrent_multimap<int, int> );
    TestTypeDefinitionPresence( concurrent_set<int> );
    TestTypeDefinitionPresence( concurrent_multiset<int> );
    TestTypeDefinitionPresence( concurrent_bounded_queue<int> );
    TestTypeDefinitionPresence( concurrent_queue<int> );
    TestTypeDefinitionPresence( concurrent_priority_queue<int> );
    TestTypeDefinitionPresence( concurrent_vector<int> );
    TestTypeDefinitionPresence( combinable<int> );
    TestTypeDefinitionPresence( enumerable_thread_specific<int> );
    /* Flow graph names */
    TestTypeDefinitionPresence( flow::graph );
    TestTypeDefinitionPresence( flow::continue_msg );
    TestTypeDefinitionPresence2(flow::tagged_msg<int, int> );
    TestFuncDefinitionPresence( flow::make_edge, (tbb::flow::sender<Msg>&, tbb::flow::receiver<Msg>&), void );
    TestFuncDefinitionPresence( flow::remove_edge, (tbb::flow::sender<Msg>&, tbb::flow::receiver<Msg>&), void );
    typedef std::tuple<int, int> intpair;
    TestTypeDefinitionPresence( flow::input_node<int> );
    TestTypeDefinitionPresence3(flow::function_node<int, int, tbb::flow::rejecting> );
    TestTypeDefinitionPresence3(flow::multifunction_node<int, intpair, tbb::flow::queueing> );
    TestTypeDefinitionPresence3(flow::async_node<int, int, tbb::flow::queueing_lightweight> );
    TestTypeDefinitionPresence2(flow::continue_node<int, tbb::flow::lightweight> );
    TestTypeDefinitionPresence2(flow::join_node<intpair, tbb::flow::reserving> );
    TestTypeDefinitionPresence2(flow::join_node<intpair, tbb::flow::key_matching<int> > );
    TestTypeDefinitionPresence( flow::split_node<intpair> );
    TestTypeDefinitionPresence( flow::overwrite_node<int> );
    TestTypeDefinitionPresence( flow::write_once_node<int> );
    TestTypeDefinitionPresence( flow::broadcast_node<int> );
    TestTypeDefinitionPresence( flow::buffer_node<int> );
    TestTypeDefinitionPresence( flow::queue_node<int> );
    TestTypeDefinitionPresence( flow::sequencer_node<int> );
    TestTypeDefinitionPresence( flow::priority_queue_node<int> );
    TestTypeDefinitionPresence( flow::limiter_node<int> );
    TestTypeDefinitionPresence2(flow::indexer_node<int, int> );
    TestTypeDefinitionPresence2(flow::composite_node<std::tuple<int>, std::tuple<int> > );
    /* Mutex names */
    TestTypeDefinitionPresence( null_mutex );
    TestTypeDefinitionPresence( null_rw_mutex );
    TestTypeDefinitionPresence( queuing_mutex );
    TestTypeDefinitionPresence( queuing_rw_mutex );
    TestTypeDefinitionPresence( spin_mutex );
    TestTypeDefinitionPresence( spin_rw_mutex );
    TestTypeDefinitionPresence( speculative_spin_mutex );
    TestTypeDefinitionPresence( speculative_spin_rw_mutex );
    TestTypeDefinitionPresence( task_group_context );
    TestTypeDefinitionPresence( task_group );
    /* Algorithm related names */
    TestTypeDefinitionPresence( blocked_range<int> );
    TestTypeDefinitionPresence( blocked_range2d<int> );
    TestTypeDefinitionPresence( blocked_range3d<int> );
    TestTypeDefinitionPresence( collaborative_once_flag );
    TestFuncDefinitionPresence( collaborative_call_once, (tbb::collaborative_once_flag&, const Body&), void );
    TestFuncDefinitionPresence( parallel_invoke, (const Body&, const Body&, const Body&), void );
    TestFuncDefinitionPresence( parallel_for_each, (int*, int*, const Body1&), void );
    TestFuncDefinitionPresence( parallel_for, (int, int, int, const Body1&), void );
    TestFuncDefinitionPresence( parallel_for, (const tbb::blocked_range<int>&, const Body2&, const tbb::simple_partitioner&), void );
    TestFuncDefinitionPresence( parallel_reduce, (const tbb::blocked_range<int>&, const int&, const Body2a&, const Body1b&), int );
    TestFuncDefinitionPresence( parallel_reduce, (const tbb::blocked_range<int>&, Body2&, tbb::affinity_partitioner&), void );
    TestFuncDefinitionPresence( parallel_deterministic_reduce, (const tbb::blocked_range<int>&, const int&, const Body2a&, const Body1b&), int );
    TestFuncDefinitionPresence( parallel_deterministic_reduce, (const tbb::blocked_range<int>&, Body2&, const tbb::static_partitioner&), void );
    TestFuncDefinitionPresence( parallel_scan, (const tbb::blocked_range2d<int>&, Body3&, const tbb::auto_partitioner&), void );
    TestFuncDefinitionPresence( parallel_scan, (const tbb::blocked_range<int>&, const int&, const Body3a&, const Body1b&), int );
    typedef int intarray[10];

    TestFuncDefinitionPresence( parallel_sort, (int*, int*), void );
    TestFuncDefinitionPresence( parallel_sort, (intarray&, const Body1bc&), void );
    TestFuncDefinitionPresence( parallel_pipeline, (size_t, const tbb::filter<void,void>&), void );
    TestFuncDefinitionPresence( parallel_invoke, (const Body&, const Body&, tbb::task_group_context&), void );
    TestFuncDefinitionPresence( parallel_for_each, (const intarray&, const Body1a&, tbb::task_group_context&), void );
    TestFuncDefinitionPresence( parallel_for, (int, int, const Body1&, const tbb::auto_partitioner&, tbb::task_group_context&), void );
    TestFuncDefinitionPresence( parallel_for, (int, int, const Body1&, tbb::task_group_context&), void );
    TestFuncDefinitionPresence( parallel_reduce, (const tbb::blocked_range<int>&, Body2&, const tbb::auto_partitioner&, tbb::task_group_context&), void );
    TestFuncDefinitionPresence( parallel_reduce, (const tbb::blocked_range<int>&, Body2&, tbb::task_group_context&), void );
    TestFuncDefinitionPresence( parallel_deterministic_reduce, (const tbb::blocked_range<int>&, Body2&, const tbb::simple_partitioner&, tbb::task_group_context&), void );
    TestFuncDefinitionPresence( parallel_deterministic_reduce, (const tbb::blocked_range<int>&, Body2&, tbb::task_group_context&), void );
    TestTypeDefinitionPresence( proportional_split );

    TestTypeDefinitionPresence( task_arena );
    TestFuncDefinitionPresence( this_task_arena::current_thread_index, (), int );
    TestFuncDefinitionPresence( this_task_arena::max_concurrency, (), int );
    TestFuncDefinitionPresence( info::numa_nodes, (), std::vector<tbb::numa_node_id> );
    TestFuncDefinitionPresence( info::default_concurrency, (tbb::numa_node_id), int );
    TestTypeDefinitionPresence( task_scheduler_observer );
    TestTypeDefinitionPresence( tbb_allocator<int> );
    TestTypeDefinitionPresence( tick_count );
    TestTypeDefinitionPresence( global_control );

#if __TBB_CPF_BUILD
    TestPreviewNames();
#endif
#ifdef DO_TEST_DEBUG_MACRO
#if TBB_USE_DEBUG
    CHECK_MESSAGE( isDebugExpected, "Debug mode is observed while release mode is expected." );
#else
    CHECK_MESSAGE( !isDebugExpected, "Release mode is observed while debug mode is expected." );
#endif /* TBB_USE_DEBUG */
#endif /* DO_TEST_DEBUG_MACRO */
    TestExceptionClassesExports();
}

#if __TBB_TEST_SECONDARY
/* This mode is used to produce a secondary object file that is linked with
   the main one in order to detect "multiple definition" linker error.
*/
void Secondary() {
    DefinitionPresence();
}
#else
//! Test for deifinition presence
//! \brief \ref interface
TEST_CASE("Test for deifinition presence") {
    DefinitionPresence();
}

void Secondary();
//! Test for "multiple definition" linker error
//! \brief \ref error_guessing
TEST_CASE("Test for multiple definition linker error") {
    Secondary();
}
#endif
