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

//! \file test_resumable_tasks.cpp
//! \brief Test for [scheduler.resumable_tasks] specification

#include "common/test.h"
#include "common/utils.h"

#include "tbb/task.h"

#if __TBB_RESUMABLE_TASKS

#include "tbb/global_control.h"
#include "tbb/task_arena.h"
#include "tbb/parallel_for.h"
#include "tbb/task_scheduler_observer.h"
#include "tbb/task_group.h"

#include <algorithm>
#include <thread>
#include <queue>
#include <condition_variable>

const int N = 10;

// External activity used in all tests, which resumes suspended execution point
class AsyncActivity {
public:
    AsyncActivity(int num_) : m_numAsyncThreads(num_) {
        for (int i = 0; i < m_numAsyncThreads ; ++i) {
            m_asyncThreads.push_back( new std::thread(AsyncActivity::asyncLoop, this) );
        }
    }
    ~AsyncActivity() {
        {
            std::lock_guard<std::mutex> lock(m_mutex);
            for (int i = 0; i < m_numAsyncThreads; ++i) {
                m_tagQueue.push(nullptr);
            }
            m_condvar.notify_all();
        }
        for (int i = 0; i < m_numAsyncThreads; ++i) {
            m_asyncThreads[i]->join();
            delete m_asyncThreads[i];
        }
        CHECK(m_tagQueue.empty());
    }
    void submit(tbb::task::suspend_point ctx) {
        std::lock_guard<std::mutex> lock(m_mutex);
        m_tagQueue.push(ctx);
        m_condvar.notify_one();
    }

private:
    static void asyncLoop(AsyncActivity* async) {
        tbb::task::suspend_point tag;
        for (;;) {
            {
                std::unique_lock<std::mutex> lock(async->m_mutex);
                async->m_condvar.wait(lock, [async] {return !async->m_tagQueue.empty(); });
                tag = async->m_tagQueue.front();
                async->m_tagQueue.pop();
            }
            if (!tag) {
                break;
            }
            tbb::task::resume(tag);
        };
    }

    const int m_numAsyncThreads;
    std::mutex m_mutex;
    std::condition_variable m_condvar;
    std::queue<tbb::task::suspend_point> m_tagQueue;
    std::vector<std::thread*> m_asyncThreads;
};

struct SuspendBody {
    SuspendBody(AsyncActivity& a_, std::thread::id id) :
        m_asyncActivity(a_), thread_id(id) {}
    void operator()(tbb::task::suspend_point tag) {
        CHECK(thread_id == std::this_thread::get_id());
        m_asyncActivity.submit(tag);
    }

private:
    AsyncActivity& m_asyncActivity;
    std::thread::id thread_id;
};

class InnermostArenaBody {
public:
    InnermostArenaBody(AsyncActivity& a_) : m_asyncActivity(a_) {}

    void operator()() {
        InnermostOuterParFor inner_outer_body(m_asyncActivity);
        tbb::parallel_for(0, N, inner_outer_body );
    }

private:
    struct InnermostInnerParFor {
        InnermostInnerParFor(AsyncActivity& a_) : m_asyncActivity(a_) {}
        void operator()(int) const {
            tbb::task::suspend(SuspendBody(m_asyncActivity, std::this_thread::get_id()));
        }
        AsyncActivity& m_asyncActivity;
    };
    struct InnermostOuterParFor {
        InnermostOuterParFor(AsyncActivity& a_) : m_asyncActivity(a_) {}
        void operator()(int) const {
            tbb::task::suspend(SuspendBody(m_asyncActivity, std::this_thread::get_id()));
            InnermostInnerParFor inner_inner_body(m_asyncActivity);
            tbb::parallel_for(0, N, inner_inner_body);
        }
        AsyncActivity& m_asyncActivity;
    };
    AsyncActivity& m_asyncActivity;
};

#include "tbb/enumerable_thread_specific.h"

class OutermostArenaBody {
public:
    OutermostArenaBody(AsyncActivity& a_, tbb::task_arena& o_, tbb::task_arena& i_
            , tbb::task_arena& id_, tbb::enumerable_thread_specific<int>& ets) :
        m_asyncActivity(a_), m_outermostArena(o_), m_innermostArena(i_), m_innermostArenaDefault(id_), m_local(ets) {}

    void operator()() {
        tbb::parallel_for(0, 32, *this);
    }

    void operator()(int i) const {
        tbb::task::suspend([&] (tbb::task::suspend_point sp) { m_asyncActivity.submit(sp); });

        tbb::task_arena& nested_arena = (i % 3 == 0) ?
            m_outermostArena : (i % 3 == 1 ? m_innermostArena : m_innermostArenaDefault);

        if (i % 3 != 0) {
            // We can only guarantee recall coorectness for "not-same" nested arenas entry
            m_local.local() = i;
        }
        InnermostArenaBody innermost_arena_body(m_asyncActivity);
        nested_arena.execute(innermost_arena_body);
        if (i % 3 != 0) {
            CHECK_MESSAGE(i == m_local.local(), "Original thread wasn't recalled for innermost nested arena.");
        }
    }

private:
    AsyncActivity& m_asyncActivity;
    tbb::task_arena& m_outermostArena;
    tbb::task_arena& m_innermostArena;
    tbb::task_arena& m_innermostArenaDefault;
    tbb::enumerable_thread_specific<int>& m_local;
};

void TestNestedArena() {
    AsyncActivity asyncActivity(4);

    tbb::task_arena outermost_arena;
    tbb::task_arena innermost_arena(2,2);
    tbb::task_arena innermost_arena_default;

    outermost_arena.initialize();
    innermost_arena_default.initialize();
    innermost_arena.initialize();

    tbb::enumerable_thread_specific<int> ets;

    OutermostArenaBody outer_arena_body(asyncActivity, outermost_arena, innermost_arena, innermost_arena_default, ets);
    outermost_arena.execute(outer_arena_body);
}

// External activity used in all tests, which resumes suspended execution point
class EpochAsyncActivity {
public:
    EpochAsyncActivity(int num_, std::atomic<int>& e_) : m_numAsyncThreads(num_), m_globalEpoch(e_) {
        for (int i = 0; i < m_numAsyncThreads ; ++i) {
            m_asyncThreads.push_back( new std::thread(EpochAsyncActivity::asyncLoop, this) );
        }
    }
    ~EpochAsyncActivity() {
        {
            std::lock_guard<std::mutex> lock(m_mutex);
            for (int i = 0; i < m_numAsyncThreads; ++i) {
                m_tagQueue.push(nullptr);
            }
            m_condvar.notify_all();
        }
        for (int i = 0; i < m_numAsyncThreads; ++i) {
            m_asyncThreads[i]->join();
            delete m_asyncThreads[i];
        }
        CHECK(m_tagQueue.empty());
    }
    void submit(tbb::task::suspend_point ctx) {
        std::lock_guard<std::mutex> lock(m_mutex);
        m_tagQueue.push(ctx);
        m_condvar.notify_one();
    }

private:
    static void asyncLoop(EpochAsyncActivity* async) {
        tbb::task::suspend_point tag;
        for (;;) {
            {
                std::unique_lock<std::mutex> lock(async->m_mutex);
                async->m_condvar.wait(lock, [async] {return !async->m_tagQueue.empty(); });
                tag = async->m_tagQueue.front();
                async->m_tagQueue.pop();
            }
            if (!tag) {
                break;
            }
            // Track the global epoch
            async->m_globalEpoch++;
            tbb::task::resume(tag);
        };
    }

    const int m_numAsyncThreads;
    std::atomic<int>& m_globalEpoch;
    std::mutex m_mutex;
    std::condition_variable m_condvar;
    std::queue<tbb::task::suspend_point> m_tagQueue;
    std::vector<std::thread*> m_asyncThreads;
};

struct EpochSuspendBody {
    EpochSuspendBody(EpochAsyncActivity& a_, std::atomic<int>& e_, int& le_) :
        m_asyncActivity(a_), m_globalEpoch(e_), m_localEpoch(le_) {}

    void operator()(tbb::task::suspend_point ctx) {
        m_localEpoch = m_globalEpoch;
        m_asyncActivity.submit(ctx);
    }

private:
    EpochAsyncActivity& m_asyncActivity;
    std::atomic<int>& m_globalEpoch;
    int& m_localEpoch;
};

// Simple test for basic resumable tasks functionality
void TestSuspendResume() {
#if __TBB_USE_SANITIZERS
    constexpr int iter_size = 100;
#else
    constexpr int iter_size = 50000;
#endif

    std::atomic<int> global_epoch; global_epoch = 0;
    EpochAsyncActivity async(4, global_epoch);

    tbb::enumerable_thread_specific<int, tbb::cache_aligned_allocator<int>, tbb::ets_suspend_aware> ets_fiber;
    std::atomic<int> inner_par_iters, outer_par_iters;
    inner_par_iters = outer_par_iters = 0;

    tbb::parallel_for(0, N, [&](int) {
        for (int i = 0; i < iter_size; ++i) {
            ets_fiber.local() = i;

            int local_epoch;
            tbb::task::suspend(EpochSuspendBody(async, global_epoch, local_epoch));
            CHECK(local_epoch < global_epoch);
            CHECK(ets_fiber.local() == i);

            tbb::parallel_for(0, N, [&](int) {
                int local_epoch2;
                tbb::task::suspend(EpochSuspendBody(async, global_epoch, local_epoch2));
                CHECK(local_epoch2 < global_epoch);
                ++inner_par_iters;
            });

            ets_fiber.local() = i;
            tbb::task::suspend(EpochSuspendBody(async, global_epoch, local_epoch));
            CHECK(local_epoch < global_epoch);
            CHECK(ets_fiber.local() == i);
        }
        ++outer_par_iters;
    });
    CHECK(outer_par_iters == N);
    CHECK(inner_par_iters == N*N*iter_size);
}

// During cleanup external thread's local task pool may
// e.g. contain proxies of affinitized tasks, but can be recalled
void TestCleanupMaster() {
    if (tbb::this_task_arena::max_concurrency() == 1) {
        // The test requires at least 2 threads
        return;
    }
    AsyncActivity asyncActivity(4);
    tbb::task_group tg;
    std::atomic<int> iter_spawned;
    std::atomic<int> iter_executed;

    for (int i = 0; i < 100; i++) {
        iter_spawned = 0;
        iter_executed = 0;

        utils::NativeParallelFor(N, [&asyncActivity, &tg, &iter_spawned, &iter_executed](int j) {
            for (int k = 0; k < j*10 + 1; ++k) {
                tg.run([&asyncActivity, j, &iter_executed] {
                    utils::doDummyWork(j * 10);
                    tbb::task::suspend(SuspendBody(asyncActivity, std::this_thread::get_id()));
                    iter_executed++;
                });
                iter_spawned++;
            }
        });
        CHECK(iter_spawned == 460);
        tg.wait();
        CHECK(iter_executed == 460);
    }
}
class ParForSuspendBody {
    AsyncActivity& asyncActivity;
    int m_numIters;
public:
    ParForSuspendBody(AsyncActivity& a_, int iters) : asyncActivity(a_), m_numIters(iters) {}
    void operator()(int) const {
        utils::doDummyWork(m_numIters);
        tbb::task::suspend(SuspendBody(asyncActivity, std::this_thread::get_id()));
    }
};

void TestNativeThread() {
    AsyncActivity asyncActivity(4);

    tbb::task_arena arena;
    tbb::task_group tg;
    std::atomic<int> iter{};
    utils::NativeParallelFor(arena.max_concurrency() / 2, [&arena, &tg, &asyncActivity, &iter](int) {
        for (int i = 0; i < 10; i++) {
            arena.execute([&tg, &asyncActivity, &iter]() {
                tg.run([&asyncActivity]() {
                    tbb::task::suspend(SuspendBody(asyncActivity, std::this_thread::get_id()));
                });
                iter++;
            });
        }
    });

    CHECK(iter == (arena.max_concurrency() / 2 * 10));
    arena.execute([&tg](){
        tg.wait();
    });
}

class ObserverTracker : public tbb::task_scheduler_observer {
    static thread_local bool is_in_arena;
public:
    std::atomic<int> counter;

    ObserverTracker(tbb::task_arena& a) : tbb::task_scheduler_observer(a) {
        counter = 0;
        observe(true);
    }
    void on_scheduler_entry(bool) override {
        bool& l = is_in_arena;
        CHECK_MESSAGE(l == false, "The thread must call on_scheduler_entry only one time.");
        l = true;
        ++counter;
    }
    void on_scheduler_exit(bool) override {
        bool& l = is_in_arena;
        CHECK_MESSAGE(l == true, "The thread must call on_scheduler_entry before calling on_scheduler_exit.");
        l = false;
    }
};

thread_local bool ObserverTracker::is_in_arena;

void TestObservers() {
    tbb::task_arena arena;
    ObserverTracker tracker(arena);
    do {
        arena.execute([] {
            tbb::parallel_for(0, 10, [](int) {
                auto thread_id = std::this_thread::get_id();
                tbb::task::suspend([thread_id](tbb::task::suspend_point tag) {
                    CHECK(thread_id == std::this_thread::get_id());
                    tbb::task::resume(tag);
                });
            }, tbb::simple_partitioner());
        });
    } while (tracker.counter < 100);
    tracker.observe(false);
}

class TestCaseGuard {
    static thread_local bool m_local;
    tbb::global_control m_threadLimit;
    tbb::global_control m_stackLimit;
public:
    TestCaseGuard()
        : m_threadLimit(tbb::global_control::max_allowed_parallelism, std::max(tbb::this_task_arena::max_concurrency(), 16))
        , m_stackLimit(tbb::global_control::thread_stack_size, 128*1024)
    {
        CHECK(m_local == false);
        m_local = true;
    }
    ~TestCaseGuard() {
        CHECK(m_local == true);
        m_local = false;
    }
};

thread_local bool TestCaseGuard::m_local = false;

//! Nested test for suspend and resume
//! \brief \ref error_guessing
TEST_CASE("Nested test for suspend and resume") {
    TestCaseGuard guard;
    TestSuspendResume();
}

//! Nested arena complex test
//! \brief \ref error_guessing
TEST_CASE("Nested arena") {
    TestCaseGuard guard;
    TestNestedArena();
}

//! Test with external threads
//! \brief \ref error_guessing
TEST_CASE("External threads") {
    TestNativeThread();
}

//! Stress test with external threads
//! \brief \ref stress
TEST_CASE("Stress test with external threads") {
    TestCleanupMaster();
}

//! Test with an arena observer
//! \brief \ref error_guessing
TEST_CASE("Arena observer") {
    TestObservers();
}
#endif /* __TBB_RESUMABLE_TASKS */
