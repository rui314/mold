/*
    Copyright (c) 2021-2022 Intel Corporation

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

#include "common/config.h"

#include <oneapi/tbb/task_arena.h>
#include <oneapi/tbb/concurrent_vector.h>
#include <oneapi/tbb/rw_mutex.h>
#include <oneapi/tbb/task_group.h>
#include <oneapi/tbb/parallel_for.h>

#include <oneapi/tbb/global_control.h>

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_concurrency_limit.h"
#include "common/spin_barrier.h"

#include <stdlib.h> // C11/POSIX aligned_alloc
#include <random>


//! \file test_scheduler_mix.cpp
//! \brief Test for [scheduler.task_arena scheduler.task_scheduler_observer] specification

const std::uint64_t maxNumActions = 1 * 100 * 1000;
static std::atomic<std::uint64_t> globalNumActions{};

//using Random = utils::FastRandom<>;
class Random {
    struct State {
        std::random_device rd;
        std::mt19937 gen;
        std::uniform_int_distribution<> dist;

        State() : gen(rd()), dist(0, std::numeric_limits<unsigned short>::max()) {}

        int get() {
            return dist(gen);
        }
    };
    static thread_local State* mState;
    tbb::concurrent_vector<State*> mStateList;
public:
    ~Random() {
        for (auto s : mStateList) {
            delete s;
        }
    }

    int get() {
        auto& s = mState;
        if (!s) {
            s = new State;
            mStateList.push_back(s);
        }
        return s->get();
    }
};

thread_local Random::State* Random::mState = nullptr;


void* aligned_malloc(std::size_t alignment, std::size_t size) {
#if _WIN32
    return _aligned_malloc(size, alignment);
#elif __unix__ || __APPLE__
    void* ptr{};
    int res = posix_memalign(&ptr, alignment, size);
    CHECK(res == 0);
    return ptr;
#else
    return aligned_alloc(alignment, size);
#endif
}

void aligned_free(void* ptr) {
#if _WIN32
    _aligned_free(ptr);
#else
    free(ptr);
#endif
}

template <typename T, std::size_t Alignment>
class PtrRWMutex {
    static const std::size_t maxThreads = (Alignment >> 1) - 1;
    static const std::uintptr_t READER_MASK = maxThreads;       // 7F..
    static const std::uintptr_t LOCKED = Alignment - 1;         // FF..
    static const std::uintptr_t LOCKED_MASK = LOCKED;           // FF..
    static const std::uintptr_t LOCK_PENDING = READER_MASK + 1; // 80..

    std::atomic<std::uintptr_t> mState;

    T* pointer() {
        return reinterpret_cast<T*>(state() & ~LOCKED_MASK);
    }

    std::uintptr_t state() {
        return mState.load(std::memory_order_relaxed);
    }

public:
    class ScopedLock {
    public:
        constexpr ScopedLock() : mMutex(nullptr), mIsWriter(false) {}
        //! Acquire lock on given mutex.
        ScopedLock(PtrRWMutex& m, bool write = true) : mMutex(nullptr) {
            CHECK_FAST(write == true);
            acquire(m);
        }
        //! Release lock (if lock is held).
        ~ScopedLock() {
            if (mMutex) {
                release();
            }
        }
        //! No Copy
        ScopedLock(const ScopedLock&) = delete;
        ScopedLock& operator=(const ScopedLock&) = delete;

        //! Acquire lock on given mutex.
        void acquire(PtrRWMutex& m) {
            CHECK_FAST(mMutex == nullptr);
            mIsWriter = true;
            mMutex = &m;
            mMutex->lock();
        }

        //! Try acquire lock on given mutex.
        bool tryAcquire(PtrRWMutex& m, bool write = true) {
            bool succeed = write ? m.tryLock() : m.tryLockShared();
            if (succeed) {
                mMutex = &m;
                mIsWriter = write;
            }
            return succeed;
        }

        void clear() {
            CHECK_FAST(mMutex != nullptr);
            CHECK_FAST(mIsWriter);
            PtrRWMutex* m = mMutex;
            mMutex = nullptr;
            m->clear();
        }

        //! Release lock.
        void release() {
            CHECK_FAST(mMutex != nullptr);
            PtrRWMutex* m = mMutex;
            mMutex = nullptr;

            if (mIsWriter) {
                m->unlock();
            }
            else {
                m->unlockShared();
            }
        }
    protected:
        PtrRWMutex* mMutex{};
        bool mIsWriter{};
    };

    bool trySet(T* ptr) {
        auto p = reinterpret_cast<std::uintptr_t>(ptr);
        CHECK_FAST((p & (Alignment - 1)) == 0);
        if (!state()) {
            std::uintptr_t expected = 0;
            if (mState.compare_exchange_strong(expected, p)) {
                return true;
            }
        }
        return false;
    }

    void clear() {
        CHECK_FAST((state() & LOCKED_MASK) == LOCKED);
        mState.store(0, std::memory_order_relaxed);
    }

    bool tryLock() {
        auto v = state();
        if (v == 0) {
            return false;
        }
        CHECK_FAST((v & LOCKED_MASK) == LOCKED || (v & READER_MASK) < maxThreads);
        if ((v & READER_MASK) == 0) {
            if (mState.compare_exchange_strong(v, v | LOCKED)) {
                return true;
            }
        }
        return false;
    }

    bool tryLockShared() {
        auto v = state();
        if (v == 0) {
            return false;
        }
        CHECK_FAST((v & LOCKED_MASK) == LOCKED || (v & READER_MASK) < maxThreads);
        if ((v & LOCKED_MASK) != LOCKED && (v & LOCK_PENDING) == 0) {
            if (mState.compare_exchange_strong(v, v + 1)) {
                return true;
            }
        }
        return false;
    }

    void lock() {
        auto v = state();
        mState.compare_exchange_strong(v, v | LOCK_PENDING);
        while (!tryLock()) {
            utils::yield();
        }
    }

    void unlock() {
        auto v = state();
        CHECK_FAST((v & LOCKED_MASK) == LOCKED);
        mState.store(v & ~LOCKED, std::memory_order_release);
    }

    void unlockShared() {
        auto v = state();
        CHECK_FAST((v & LOCKED_MASK) != LOCKED);
        CHECK_FAST((v & READER_MASK) > 0);
        mState -= 1;
    }

    operator bool() const {
        return pointer() != 0;
    }

    T* get() {
        return pointer();
    }
};

class Statistics {
public:
    enum ACTION {
        ArenaCreate,
        ArenaDestroy,
        ArenaAcquire,
        skippedArenaCreate,
        skippedArenaDestroy,
        skippedArenaAcquire,
        ParallelAlgorithm,
        ArenaEnqueue,
        ArenaExecute,
        numActions
    };

    static const char* const mStatNames[numActions];
private:
    struct StatType {
        StatType() : mCounters() {}
        std::array<std::uint64_t, numActions> mCounters;
    };

    tbb::concurrent_vector<StatType*> mStatsList;
    static thread_local StatType* mStats;

    StatType& get() {
        auto& s = mStats;
        if (!s) {
            s = new StatType;
            mStatsList.push_back(s);
        }
        return *s;
    }
public:
    ~Statistics() {
        for (auto s : mStatsList) {
            delete s;
        }
    }

    void notify(ACTION a) {
        ++get().mCounters[a];
    }

    void report() {
        StatType summary;
        for (auto& s : mStatsList) {
            for (int i = 0; i < numActions; ++i) {
                summary.mCounters[i] += s->mCounters[i];
            }
        }
        std::cout << std::endl << "Statistics:" << std::endl;
        std::cout << "Total actions: " << globalNumActions << std::endl;
        for (int i = 0; i < numActions; ++i) {
            std::cout << mStatNames[i] << ": " << summary.mCounters[i] << std::endl;
        }
    }
};


const char* const Statistics::mStatNames[Statistics::numActions] = {
    "Arena create", "Arena destroy", "Arena acquire",
    "Skipped arena create", "Skipped arena destroy", "Skipped arena acquire",
    "Parallel algorithm", "Arena enqueue", "Arena execute"
};
thread_local Statistics::StatType* Statistics::mStats;

static Statistics gStats;

class LifetimeTracker {
public:
    LifetimeTracker() = default;

    class Guard {
    public:
        Guard(LifetimeTracker* obj) {
            if (!(obj->mReferences.load(std::memory_order_relaxed) & SHUTDOWN_FLAG)) {
                if (obj->mReferences.fetch_add(REFERENCE_FLAG) & SHUTDOWN_FLAG) {
                    obj->mReferences.fetch_sub(REFERENCE_FLAG);
                } else {
                    mObj = obj;
                }
            }
        }

        Guard(Guard&& ing) : mObj(ing.mObj) {
            ing.mObj = nullptr;
        }

        ~Guard() {
            if (mObj != nullptr) {
                mObj->mReferences.fetch_sub(REFERENCE_FLAG);
            }
        }

        bool continue_execution() {
            return mObj != nullptr;
        }

    private:
        LifetimeTracker* mObj{nullptr};
    };

    Guard makeGuard() {
        return Guard(this);
    }

    void signalShutdown() {
        mReferences.fetch_add(SHUTDOWN_FLAG);
    }

    void waitCompletion() {
        utils::SpinWaitUntilEq(mReferences, SHUTDOWN_FLAG);
    }

private:
    friend class Guard;
    static constexpr std::uintptr_t SHUTDOWN_FLAG = 1;
    static constexpr std::uintptr_t REFERENCE_FLAG = 1 << 1;
    std::atomic<std::uintptr_t> mReferences{};
};

class ArenaTable {
    static const std::size_t maxArenas = 64;
    static const std::size_t maxThreads = 1 << 9;
    static const std::size_t arenaAligment = maxThreads << 1;

    using ArenaPtrRWMutex = PtrRWMutex<tbb::task_arena, arenaAligment>;
    std::array<ArenaPtrRWMutex, maxArenas> mArenaTable;

    struct ThreadState {
        bool lockedArenas[maxArenas]{};
        int arenaIdxStack[maxArenas];
        int level{};
    };

    LifetimeTracker mLifetimeTracker{};
    static thread_local ThreadState mThreadState;

    template <typename F>
    auto find_arena(std::size_t start, F f) -> decltype(f(std::declval<ArenaPtrRWMutex&>(), std::size_t{})) {
        for (std::size_t idx = start, i = 0; i < maxArenas; ++i, idx = (idx + 1) % maxArenas) {
            auto res = f(mArenaTable[idx], idx);
            if (res) {
                return res;
            }
        }
        return {};
    }

public:
    using ScopedLock = ArenaPtrRWMutex::ScopedLock;

    void create(Random& rnd) {
        auto guard = mLifetimeTracker.makeGuard();
        if (guard.continue_execution()) {
            int num_threads = rnd.get() % utils::get_platform_max_threads() + 1;
            unsigned int num_reserved = rnd.get() % num_threads;
            tbb::task_arena::priority priorities[] = { tbb::task_arena::priority::low , tbb::task_arena::priority::normal, tbb::task_arena::priority::high };
            tbb::task_arena::priority priority = priorities[rnd.get() % 3];

            tbb::task_arena* a = new (aligned_malloc(arenaAligment, arenaAligment)) tbb::task_arena{ num_threads , num_reserved , priority };

            if (!find_arena(rnd.get() % maxArenas, [a](ArenaPtrRWMutex& arena, std::size_t) -> bool {
                    if (arena.trySet(a)) {
                        return true;
                    }
                    return false;
                }))
            {
                gStats.notify(Statistics::skippedArenaCreate);
                a->~task_arena();
                aligned_free(a);
            }
        }
    }

    void destroy(Random& rnd) {
        auto guard = mLifetimeTracker.makeGuard();
        if (guard.continue_execution()) {
            auto& ts = mThreadState;
            if (!find_arena(rnd.get() % maxArenas, [&ts](ArenaPtrRWMutex& arena, std::size_t idx) {
                    if (!ts.lockedArenas[idx]) {
                        ScopedLock lock;
                        if (lock.tryAcquire(arena, true)) {
                            auto a = arena.get();
                            lock.clear();
                            a->~task_arena();
                            aligned_free(a);
                            return true;
                        }
                    }
                    return false;
                }))
            {
                gStats.notify(Statistics::skippedArenaDestroy);
            }
        }
    }

    void shutdown() {
        mLifetimeTracker.signalShutdown();
        mLifetimeTracker.waitCompletion();
        find_arena(0, [](ArenaPtrRWMutex& arena, std::size_t) {
            if (arena.get()) {
                ScopedLock lock{ arena, true };
                auto a = arena.get();
                lock.clear();
                a->~task_arena();
                aligned_free(a);
            }
            return false;
        });
    }

    std::pair<tbb::task_arena*, std::size_t> acquire(Random& rnd, ScopedLock& lock) {
        auto guard = mLifetimeTracker.makeGuard();

        tbb::task_arena* a{nullptr};
        std::size_t resIdx{};
        if (guard.continue_execution()) {
            auto& ts = mThreadState;
            a = find_arena(rnd.get() % maxArenas,
                [&ts, &lock, &resIdx](ArenaPtrRWMutex& arena, std::size_t idx) -> tbb::task_arena* {
                    if (!ts.lockedArenas[idx]) {
                        if (lock.tryAcquire(arena, false)) {
                            ts.lockedArenas[idx] = true;
                            ts.arenaIdxStack[ts.level++] = int(idx);
                            resIdx = idx;
                            return arena.get();
                        }
                    }
                    return nullptr;
                });
            if (!a) {
                gStats.notify(Statistics::skippedArenaAcquire);
            }
        }
        return { a, resIdx };
    }

    void release(ScopedLock& lock) {
        auto& ts = mThreadState;
        CHECK_FAST(ts.level > 0);
        auto idx = ts.arenaIdxStack[--ts.level];
        CHECK_FAST(ts.lockedArenas[idx]);
        ts.lockedArenas[idx] = false;
        lock.release();
    }
};

thread_local ArenaTable::ThreadState ArenaTable::mThreadState;

static ArenaTable arenaTable;
static Random threadRandom;

enum ACTIONS {
    arena_create,
    arena_destroy,
    arena_action,
    parallel_algorithm,
    // TODO:
    // observer_attach,
    // observer_detach,
    // flow_graph,
    // task_group,
    // resumable_tasks,

    num_actions
};

void global_actor();

template <ACTIONS action>
struct actor;

template <>
struct actor<arena_create> {
    static void do_it(Random& r) {
        arenaTable.create(r);
    }
};

template <>
struct actor<arena_destroy> {
    static void do_it(Random& r) {
        arenaTable.destroy(r);
    }
};

template <>
struct actor<arena_action> {
    static void do_it(Random& r) {
        static thread_local std::size_t arenaLevel = 0;
        ArenaTable::ScopedLock lock;
        auto entry = arenaTable.acquire(r, lock);
        if (entry.first) {
            enum arena_actions {
                arena_execute,
                arena_enqueue,
                num_arena_actions
            };
            auto process = r.get() % 2;
            auto body = [process] {
                if (process) {
                    tbb::detail::d1::wait_context wctx{ 1 };
                    tbb::task_group_context ctx;
                    tbb::this_task_arena::enqueue([&wctx] { wctx.release(); });
                    tbb::detail::d1::wait(wctx, ctx);
                } else {
                    global_actor();
                }
            };
            switch (r.get() % (16*num_arena_actions)) {
            case arena_execute:
                if (entry.second > arenaLevel) {
                    gStats.notify(Statistics::ArenaExecute);
                    auto oldArenaLevel = arenaLevel;
                    arenaLevel = entry.second;
                    entry.first->execute(body);
                    arenaLevel = oldArenaLevel;
                    break;
                }
                utils_fallthrough;
            case arena_enqueue:
                utils_fallthrough;
            default:
                gStats.notify(Statistics::ArenaEnqueue);
                entry.first->enqueue([] { global_actor(); });
                break;
            }
            arenaTable.release(lock);
        }
    }
};

template <>
struct actor<parallel_algorithm> {
    static void do_it(Random& rnd) {
        enum PARTITIONERS {
            simpl_part,
            auto_part,
            aff_part,
            static_part,
            num_parts
        };
        int sz = rnd.get() % 10000;
        auto doGlbAction = rnd.get() % 1000 == 42;
        auto body = [doGlbAction, sz](int i) {
            if (i == sz / 2 && doGlbAction) {
                global_actor();
            }
        };

        switch (rnd.get() % num_parts) {
        case simpl_part:
            tbb::parallel_for(0, sz, body, tbb::simple_partitioner{}); break;
        case auto_part:
            tbb::parallel_for(0, sz, body, tbb::auto_partitioner{}); break;
        case aff_part:
        {
            tbb::affinity_partitioner aff;
            tbb::parallel_for(0, sz, body, aff); break;
        }
        case static_part:
            tbb::parallel_for(0, sz, body, tbb::static_partitioner{}); break;
        }
    }
};

void global_actor() {
    static thread_local std::uint64_t localNumActions{};

    while (globalNumActions < maxNumActions) {
        auto& rnd = threadRandom;
        switch (rnd.get() % num_actions) {
        case arena_create:  gStats.notify(Statistics::ArenaCreate); actor<arena_create>::do_it(rnd);  break;
        case arena_destroy: gStats.notify(Statistics::ArenaDestroy); actor<arena_destroy>::do_it(rnd); break;
        case arena_action:  gStats.notify(Statistics::ArenaAcquire); actor<arena_action>::do_it(rnd);  break;
        case parallel_algorithm: gStats.notify(Statistics::ParallelAlgorithm); actor<parallel_algorithm>::do_it(rnd);  break;
        }

        if (++localNumActions == 100) {
            localNumActions = 0;
            globalNumActions += 100;

            // TODO: Enable statistics
            // static std::mutex mutex;
            // std::lock_guard<std::mutex> lock{ mutex };
            // std::cout << globalNumActions << "\r" << std::flush;
        }
    }
}

#if TBB_USE_EXCEPTIONS
//! \brief \ref stress
TEST_CASE("Stress test with mixing functionality") {
    // TODO add thread recreation
    // TODO: Enable statistics
    tbb::task_scheduler_handle handle{ tbb::attach{} };

    const std::size_t numExtraThreads = 16;
    utils::SpinBarrier startBarrier{numExtraThreads};
    utils::NativeParallelFor(numExtraThreads, [&startBarrier](std::size_t) {
        startBarrier.wait();
        global_actor();
    });

    arenaTable.shutdown();

    tbb::finalize(handle);

    // gStats.report();
}
#endif
