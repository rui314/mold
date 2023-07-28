/*
    Copyright (c) 2005-2022 Intel Corporation

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

//! \file test_tbb_fork.cpp
//! \brief Test for [sched.global_control] specification

#include "tbb/global_control.h"
#include "tbb/blocked_range.h"
#include "tbb/cache_aligned_allocator.h"
#include "tbb/parallel_for.h"

static const int MinThread = 1;
static const int MaxThread = 4;

// Doctest is not used here, but placed just to prevent compiler errors for bad headers design
#define DOCTEST_CONFIG_IMPLEMENT
#include "common/test.h"

#include "common/utils.h"
#include "common/utils_assert.h"

#if _WIN32||_WIN64
#include "tbb/concurrent_hash_map.h"

HANDLE getCurrentThreadHandle()
{
    HANDLE hProc = GetCurrentProcess(), hThr = INVALID_HANDLE_VALUE;
#if TBB_USE_ASSERT
    BOOL res =
#endif
    DuplicateHandle( hProc, GetCurrentThread(), hProc, &hThr, 0, FALSE, DUPLICATE_SAME_ACCESS );
    __TBB_ASSERT( res, "Retrieving current thread handle failed" );
    return hThr;
}

bool threadTerminated(HANDLE h)
{
    DWORD ret = WaitForSingleObjectEx(h, 0, FALSE);
    return WAIT_OBJECT_0 == ret;
}

struct Data {
    HANDLE h;
};

typedef tbb::concurrent_hash_map<DWORD, Data> TidTableType;

static TidTableType tidTable;

#else

#if __sun || __SUNPRO_CC
#define _POSIX_PTHREAD_SEMANTICS 1 // to get standard-conforming sigwait(2)
#endif
#include <signal.h>
#include <sys/types.h>
#include <unistd.h>
#include <sys/wait.h>
#include <sched.h>

#include "tbb/tick_count.h"

void SigHandler(int) { }

#endif // _WIN32||_WIN64

class AllocTask {
public:
    void operator() (const tbb::blocked_range<int> &r) const {
#if _WIN32||_WIN64
        HANDLE h = getCurrentThreadHandle();
        DWORD tid = GetCurrentThreadId();
        {
            TidTableType::accessor acc;
            if (tidTable.insert(acc, tid)) {
                acc->second.h = h;
            }
        }
#endif
        for (int y = r.begin(); y != r.end(); ++y) {
            void *p = tbb::detail::r1::cache_aligned_allocate(7000);
            tbb::detail::r1::cache_aligned_deallocate(p);
        }
    }
    AllocTask() {}
};

void CallParallelFor()
{
    tbb::parallel_for(tbb::blocked_range<int>(0, 10000, 1), AllocTask(),
                      tbb::simple_partitioner());
}

/* Regression test against data race between termination of workers
   and setting blocking termination mode in main thread. */
class RunWorkersBody : utils::NoAssign {
    bool wait_workers;
public:
    RunWorkersBody(bool waitWorkers) : wait_workers(waitWorkers) {}
    void operator()(const int /*threadID*/) const {
        tbb::task_scheduler_handle tsi{tbb::attach{}};
        CallParallelFor();
        if (wait_workers) {
            bool ok = tbb::finalize(tsi, std::nothrow);
            ASSERT(ok, nullptr);
        } else {
            tsi.release();
        }
    }
};

void TestBlockNonblock()
{
    for (int i=0; i<100; i++) {
        utils::NativeParallelFor(4, RunWorkersBody(/*wait_workers=*/false));
        RunWorkersBody(/*wait_workers=*/true)(0);
    }
}

class RunInNativeThread : utils::NoAssign {
    bool blocking;

public:
    RunInNativeThread(bool blocking_) : blocking(blocking_) {}
    void operator()(const int /*threadID*/) const {
        tbb::task_scheduler_handle tsi = tbb::task_scheduler_handle{tbb::attach{}};
        CallParallelFor();
        if (blocking) {
            bool ok = tbb::finalize(tsi, std::nothrow);
            ASSERT(!ok, "Nested blocking terminate must fail.");
        } else {
            tsi.release();
        }
    }
};

void TestTasksInThread()
{
    tbb::task_scheduler_handle sch{tbb::attach{}};
    CallParallelFor();
    utils::NativeParallelFor(2, RunInNativeThread(/*blocking=*/false));
    bool ok = tbb::finalize(sch, std::nothrow);
    ASSERT(ok, nullptr);
}

#if TBB_REVAMP_TODO

#include "common/memory_usage.h"

// check for memory leak during TBB task scheduler init/terminate life cycle
// TODO: move to test_task_scheduler_init after workers waiting productization
void TestSchedulerMemLeaks()
{
    const int ITERS = 10;
    int it;

    for (it=0; it<ITERS; it++) {
        size_t memBefore = utils::GetMemoryUsage();
#if _MSC_VER && _DEBUG
        // _CrtMemCheckpoint() and _CrtMemDifference are non-empty only in _DEBUG
        _CrtMemState stateBefore, stateAfter, diffState;
        _CrtMemCheckpoint(&stateBefore);
#endif
        for (int i=0; i<100; i++) {
            tbb::task_arena arena(1, 1); arena.initialize(); // right approach?
            // tbb::task_scheduler_init sch(1);
            tbb::task_scheduler_handle sch = tbb::task_scheduler_handle::get();
            for (int k=0; k<10; k++) {
                // tbb::empty_task *t = new( tbb::task::allocate_root() ) tbb::empty_task();
                // tbb::task::enqueue(*t);
                arena.enqueue([&]{});
            }
            bool ok = tbb::finalize(sch, std::nothrow);
            ASSERT(ok, nullptr);
        }
#if _MSC_VER && _DEBUG
        _CrtMemCheckpoint(&stateAfter);
        int ret = _CrtMemDifference(&diffState, &stateBefore, &stateAfter);
        ASSERT(!ret, "It must be no memory leaks at this point.");
#endif
        if (utils::GetMemoryUsage() <= memBefore)
            break;
    }
    ASSERT(it < ITERS, "Memory consumption has not stabilized. Memory Leak?");
}

#endif // TBB_REVAMP_TODO

void TestNestingTSI()
{
    // nesting with and without blocking is possible
    for (int i=0; i<2; i++) {
        tbb::task_scheduler_handle schBlock = tbb::task_scheduler_handle{tbb::attach{}};
        CallParallelFor();
        tbb::task_scheduler_handle schBlock1;
        schBlock1 = tbb::task_scheduler_handle{tbb::attach{}};
        CallParallelFor();
        if (i) {
            schBlock1.release();
        } else {
            bool ok = tbb::finalize(schBlock1, std::nothrow);
            ASSERT(!ok, "Nested blocking terminate must fail.");
        }
        bool ok = tbb::finalize(schBlock, std::nothrow);
        ASSERT(ok, nullptr);
    }
    {
        tbb::task_scheduler_handle schBlock{tbb::attach{}};
        utils::NativeParallelFor(1, RunInNativeThread(/*blocking=*/true));
        bool ok = tbb::finalize(schBlock, std::nothrow);
        ASSERT(ok, nullptr);
    }
}

void TestAutoInit()
{
    CallParallelFor(); // autoinit
    // creation of blocking scheduler is possible, but one is not block
    utils::NativeParallelFor(1, RunInNativeThread(/*blocking=*/true));
}

int main()
{
    TestNestingTSI();
    TestBlockNonblock();
    TestTasksInThread();

#if TBB_REVAMP_TODO
    TestSchedulerMemLeaks();
#endif

    bool child = false;
#if _WIN32||_WIN64
    DWORD masterTid = GetCurrentThreadId();
#else
    struct sigaction sa;
    sigset_t sig_set;

    sigemptyset(&sa.sa_mask);
    sa.sa_flags = 0;
    sa.sa_handler = SigHandler;
    if (sigaction(SIGCHLD, &sa, nullptr))
        ASSERT(0, "sigaction failed");
    if (sigaction(SIGALRM, &sa, nullptr))
        ASSERT(0, "sigaction failed");
    // block SIGCHLD and SIGALRM, the mask is inherited by worker threads
    sigemptyset(&sig_set);
    sigaddset(&sig_set, SIGCHLD);
    sigaddset(&sig_set, SIGALRM);
    if (pthread_sigmask(SIG_BLOCK, &sig_set, nullptr))
        ASSERT(0, "pthread_sigmask failed");
#endif
    utils::suppress_unused_warning(child);
    for (int threads=MinThread; threads<=MaxThread; threads+=MinThread) {
        for (int i=0; i<20; i++) {
            tbb::global_control ctl(tbb::global_control::max_allowed_parallelism, threads);
            {
                tbb::task_scheduler_handle sch{tbb::attach{}};
                bool ok = tbb::finalize( sch, std::nothrow );
                ASSERT(ok, nullptr);
            }
            tbb::task_scheduler_handle sch{tbb::attach{}};
            CallParallelFor();
            bool ok = tbb::finalize( sch, std::nothrow );
            ASSERT(ok, nullptr);
#if _WIN32||_WIN64
            // check that there is no alive threads after terminate()
            for (TidTableType::const_iterator it = tidTable.begin();
                it != tidTable.end(); ++it) {
                if (masterTid != it->first) {
                    ASSERT(threadTerminated(it->second.h), nullptr);
                }
            }
            tidTable.clear();
#else // _WIN32||_WIN64
            if (child)
                exit(0);
            else {
                pid_t pid = fork();
                if (!pid) {
                    i = -1;
                    child = true;
                } else {
                    int sig;
                    pid_t w_ret = 0;
                    // wait for SIGCHLD up to timeout
                    alarm(30);
                    if (0 != sigwait(&sig_set, &sig))
                        ASSERT(0, "sigwait failed");
                    alarm(0);
                    w_ret = waitpid(pid, nullptr, WNOHANG);
                    ASSERT(w_ret>=0, "waitpid failed");
                    if (!w_ret) {
                        ASSERT(!kill(pid, SIGKILL), nullptr);
                        w_ret = waitpid(pid, nullptr, 0);
                        ASSERT(w_ret!=-1, "waitpid failed");

                        ASSERT(0, "Hang after fork");
                    }
                    // clean pending signals (if any occurs since sigwait)
                    sigset_t p_mask;
                    for (;;) {
                        sigemptyset(&p_mask);
                        sigpending(&p_mask);
                        if (sigismember(&p_mask, SIGALRM)
                            || sigismember(&p_mask, SIGCHLD)) {
                            if (0 != sigwait(&p_mask, &sig))
                                ASSERT(0, "sigwait failed");
                        } else
                            break;
                    }
                }
            }
#endif // _WIN32||_WIN64
        }
    }
    // auto initialization at this point
    TestAutoInit();

    return 0;
}
