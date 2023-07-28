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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include "conformance_mutex.h"

#include "oneapi/tbb/spin_mutex.h"
#include "oneapi/tbb/mutex.h"
#include "oneapi/tbb/spin_rw_mutex.h"
#include "oneapi/tbb/rw_mutex.h"
#include "oneapi/tbb/queuing_mutex.h"
#include "oneapi/tbb/queuing_rw_mutex.h"
#include "oneapi/tbb/null_mutex.h"
#include "oneapi/tbb/null_rw_mutex.h"

//! \file conformance_mutex.cpp
//! \brief Test for [mutex.spin_mutex mutex.spin_rw_mutex mutex.queuing_mutex mutex.queuing_rw_mutex mutex.mutex mutex.rw_mutex mutex.speculative_spin_mutex mutex.speculative_spin_rw_mutex mutex.null_mutex mutex.null_rw_mutex] specifications

//! Testing Mutex requirements
//! \brief \ref interface \ref requirement
TEST_CASE("Basic Locable requirement test") {
    // BasicLockable
    GeneralTest<oneapi::tbb::spin_mutex>("Spin Mutex");
    GeneralTest<oneapi::tbb::spin_rw_mutex>("Spin RW Mutex");
    GeneralTest<oneapi::tbb::queuing_mutex>("Queuing Mutex");
    GeneralTest<oneapi::tbb::queuing_rw_mutex>("Queuing RW Mutex");
    GeneralTest<oneapi::tbb::mutex>("Adaptive Mutex");
    GeneralTest<oneapi::tbb::rw_mutex>("Adaptive RW Mutex");
    // TODO: Consider adding Thread Sanitizer (note that accesses inside the transaction
    // considered as races by Thread Sanitizer)
#if !__TBB_USE_THREAD_SANITIZER
    GeneralTest<oneapi::tbb::speculative_spin_mutex>("Speculative Spin Mutex");
    GeneralTest<oneapi::tbb::speculative_spin_rw_mutex>("Speculative Spin RW Mutex");
#endif
    // NullMutexes
    GeneralTest<oneapi::tbb::null_mutex, utils::AtomicCounter<oneapi::tbb::null_mutex>>("Null Mutex", false);
    GeneralTest<oneapi::tbb::null_rw_mutex, utils::AtomicCounter<oneapi::tbb::null_rw_mutex>>("Null RW Mutex", false);
    TestNullMutex<oneapi::tbb::null_mutex>("Null Mutex");
    TestNullMutex<oneapi::tbb::null_rw_mutex>("Null RW Mutex");
}

//! \brief \ref interface \ref requirement
TEST_CASE("Lockable requirement test") {
    // Lockable - single threaded try_acquire operations
    TestTryAcquire<oneapi::tbb::spin_mutex>("Spin Mutex");
    TestTryAcquire<oneapi::tbb::spin_rw_mutex>("Spin RW Mutex");
    TestTryAcquire<oneapi::tbb::queuing_mutex>("Queuing Mutex");
    TestTryAcquire<oneapi::tbb::queuing_rw_mutex>("Queuing RW Mutex");
    TestTryAcquire<oneapi::tbb::mutex>("Adaptive Mutex");
    TestTryAcquire<oneapi::tbb::rw_mutex>("Adaptive RW Mutex");
#if !__TBB_USE_THREAD_SANITIZER
    TestTryAcquire<oneapi::tbb::speculative_spin_mutex>("Speculative Spin Mutex");
    TestTryAcquire<oneapi::tbb::speculative_spin_rw_mutex>("Speculative Spin RW Mutex");
#endif
    TestTryAcquire<oneapi::tbb::null_mutex>("Null Mutex");
}

//! Testing ReaderWriterMutex requirements
//! \brief \ref interface \ref requirement
TEST_CASE("Shared mutexes (reader/writer) test") {
    // General reader writer capabilities + upgrade/downgrade
    TestReaderWriterLock<oneapi::tbb::spin_rw_mutex>("Spin RW Mutex");
    TestReaderWriterLock<oneapi::tbb::queuing_rw_mutex>("Queuing RW Mutex");
    TestReaderWriterLock<oneapi::tbb::rw_mutex>("Adaptive RW Mutex");
    TestNullRWMutex<oneapi::tbb::null_rw_mutex>("Null RW Mutex");
    // Single threaded read/write try_acquire operations
    TestTryAcquireReader<oneapi::tbb::spin_rw_mutex>("Spin RW Mutex");
    TestTryAcquireReader<oneapi::tbb::queuing_rw_mutex>("Queuing RW Mutex");
    TestRWStateMultipleChange<oneapi::tbb::spin_rw_mutex>("Spin RW Mutex");
    TestRWStateMultipleChange<oneapi::tbb::queuing_rw_mutex>("Queuing RW Mutex");
    TestRWStateMultipleChange<oneapi::tbb::rw_mutex>("Adaptive RW Mutex");
    TestTryAcquireReader<oneapi::tbb::null_rw_mutex>("Null RW Mutex");
#if !__TBB_USE_THREAD_SANITIZER
    TestReaderWriterLock<oneapi::tbb::speculative_spin_rw_mutex>("Speculative Spin RW Mutex");
    TestTryAcquireReader<oneapi::tbb::speculative_spin_rw_mutex>("Speculative Spin RW Mutex");
    TestRWStateMultipleChange<oneapi::tbb::speculative_spin_rw_mutex>("Speculative Spin RW Mutex");
#endif
}

//! Testing ISO C++ Mutex and Shared Mutex requirements.
//! Compatibility with the standard
//! \brief \ref interface \ref requirement
TEST_CASE("ISO interface test") {
    GeneralTest<TBB_MutexFromISO_Mutex<oneapi::tbb::spin_mutex> >("ISO Spin Mutex");
    GeneralTest<TBB_MutexFromISO_Mutex<oneapi::tbb::spin_rw_mutex> >("ISO Spin RW Mutex");
    GeneralTest<TBB_MutexFromISO_Mutex<oneapi::tbb::mutex> >("ISO Adaprive Mutex");
    GeneralTest<TBB_MutexFromISO_Mutex<oneapi::tbb::rw_mutex>>("ISO Adaptive RW Mutex");
    TestTryAcquire<TBB_MutexFromISO_Mutex<oneapi::tbb::spin_mutex> >("ISO Spin Mutex");
    TestTryAcquire<TBB_MutexFromISO_Mutex<oneapi::tbb::spin_rw_mutex> >("ISO Spin RW Mutex");
    TestTryAcquire<TBB_MutexFromISO_Mutex<oneapi::tbb::mutex> >("ISO Adaptive  Mutex");
    TestTryAcquire<TBB_MutexFromISO_Mutex<oneapi::tbb::rw_mutex>>("ISO Adaptive RW Mutex");
    TestTryAcquireReader<TBB_MutexFromISO_Mutex<oneapi::tbb::spin_rw_mutex> >("ISO Spin RW Mutex");
    TestReaderWriterLock<TBB_MutexFromISO_Mutex<oneapi::tbb::spin_rw_mutex> >("ISO Spin RW Mutex");
    TestTryAcquireReader<TBB_MutexFromISO_Mutex<oneapi::tbb::rw_mutex>>("ISO Adaptive RW Mutex");
    TestReaderWriterLock<TBB_MutexFromISO_Mutex<oneapi::tbb::rw_mutex>>("ISO Adaptive RW Mutex");
}
