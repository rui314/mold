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

#define TBB_PREVIEW_MUTEXES 1

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include "../conformance/conformance_mutex.h"
#include "test_mutex.h"

#include "oneapi/tbb/mutex.h"
#include "oneapi/tbb/rw_mutex.h"

//! \file test_adaptive_mutex.cpp
//! \brief Test for [preview] functionality

//! \brief \ref error_guessing
TEST_CASE("test upgrade/downgrade with rw_mutex") {
    test_rwm_upgrade_downgrade<tbb::rw_mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test mutex with native threads") {
    test_with_native_threads::test<tbb::mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test rw_mutex with native threads") {
    test_with_native_threads::test<tbb::rw_mutex>();
    test_with_native_threads::test_rw<tbb::rw_mutex>();
}

//! Testing Mutex requirements
//! \brief \ref interface \ref requirement
TEST_CASE("Basic Locable requirement test") {
    // BasicLockable
    GeneralTest<oneapi::tbb::mutex>("Adaptive Mutex");
    GeneralTest<oneapi::tbb::rw_mutex>("Adaptive RW Mutex");
}

//! \brief \ref interface \ref requirement
TEST_CASE("Lockable requirement test") {
    // Lockable - single threaded try_acquire operations
    TestTryAcquire<oneapi::tbb::mutex>("Adaptive Mutex");
    TestTryAcquire<oneapi::tbb::rw_mutex>("Adaptive RW Mutex");
}

//! Testing ReaderWriterMutex requirements
//! \brief \ref interface \ref requirement
TEST_CASE("Shared mutexes (reader/writer) test") {
    // General reader writer capabilities + upgrade/downgrade
    TestReaderWriterLock<oneapi::tbb::rw_mutex>("Adaptive RW Mutex");
    TestRWStateMultipleChange<oneapi::tbb::rw_mutex>("Adaptive RW Mutex");
}

//! Testing ISO C++ Mutex and Shared Mutex requirements.
//! Compatibility with the standard
//! \brief \ref interface \ref requirement
TEST_CASE("ISO interface test") {
    GeneralTest<TBB_MutexFromISO_Mutex<oneapi::tbb::mutex> >("ISO Adaprive Mutex");
    GeneralTest<TBB_MutexFromISO_Mutex<oneapi::tbb::rw_mutex>>("ISO Adaptive RW Mutex");
    TestTryAcquire<TBB_MutexFromISO_Mutex<oneapi::tbb::mutex> >("ISO Adaptive  Mutex");
    TestTryAcquire<TBB_MutexFromISO_Mutex<oneapi::tbb::rw_mutex>>("ISO Adaptive RW Mutex");
    TestTryAcquireReader<TBB_MutexFromISO_Mutex<oneapi::tbb::rw_mutex>>("ISO Adaptive RW Mutex");
    TestReaderWriterLock<TBB_MutexFromISO_Mutex<oneapi::tbb::rw_mutex>>("ISO Adaptive RW Mutex");
}

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("Test internal mutex concepts for oneapi::tbb::mutex and oneapi::tbb::rw_mutex") {
    static_assert(oneapi::tbb::detail::scoped_lockable<oneapi::tbb::mutex>);
    static_assert(oneapi::tbb::detail::scoped_lockable<oneapi::tbb::rw_mutex>);
    static_assert(oneapi::tbb::detail::rw_scoped_lockable<oneapi::tbb::rw_mutex>);
}
#endif
