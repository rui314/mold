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

//! \file test_malloc_pools.cpp
//! \brief Test for [memory_allocation] functionality

#define __TBB_NO_IMPLICIT_LINKAGE 1

#include "common/test.h"

#define HARNESS_TBBMALLOC_THREAD_SHUTDOWN 1

#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/spin_barrier.h"
#include "common/tls_limit.h"

#include "tbb/scalable_allocator.h"

#include <atomic>

template<typename T>
static inline T alignUp  (T arg, uintptr_t alignment) {
    return T(((uintptr_t)arg+(alignment-1)) & ~(alignment-1));
}

struct PoolSpace: utils::NoCopy {
    size_t pos;
    int    regions;
    size_t bufSize;
    char  *space;

    static const size_t BUF_SIZE = 8*1024*1024;

    PoolSpace(size_t bufSz = BUF_SIZE) :
        pos(0), regions(0),
        bufSize(bufSz), space(new char[bufSize]) {
        memset(space, 0, bufSize);
    }
    ~PoolSpace() {
        delete []space;
    }
};

static PoolSpace *poolSpace;

struct MallocPoolHeader {
    void  *rawPtr;
    size_t userSize;
};

static std::atomic<int> liveRegions;

static void *getMallocMem(intptr_t /*pool_id*/, size_t &bytes)
{
    void *rawPtr = malloc(bytes+sizeof(MallocPoolHeader)+1);
    if (!rawPtr)
        return nullptr;
    // +1 to check working with unaligned space
    void *ret = (void *)((uintptr_t)rawPtr+sizeof(MallocPoolHeader)+1);

    MallocPoolHeader *hdr = (MallocPoolHeader*)ret-1;
    hdr->rawPtr = rawPtr;
    hdr->userSize = bytes;

    liveRegions++;

    return ret;
}

static int putMallocMem(intptr_t /*pool_id*/, void *ptr, size_t bytes)
{
    MallocPoolHeader *hdr = (MallocPoolHeader*)ptr-1;
    ASSERT(bytes == hdr->userSize, "Invalid size in pool callback.");
    free(hdr->rawPtr);

    liveRegions--;

    return 0;
}

void TestPoolReset()
{
    rml::MemPoolPolicy pol(getMallocMem, putMallocMem);
    rml::MemoryPool *pool;

    pool_create_v1(0, &pol, &pool);
    for (int i=0; i<100; i++) {
        REQUIRE(pool_malloc(pool, 8));
        REQUIRE(pool_malloc(pool, 50*1024));
    }
    int regionsBeforeReset = liveRegions.load(std::memory_order_acquire);
    bool ok = pool_reset(pool);
    REQUIRE(ok);
    for (int i=0; i<100; i++) {
        REQUIRE(pool_malloc(pool, 8));
        REQUIRE(pool_malloc(pool, 50*1024));
    }
    REQUIRE_MESSAGE(regionsBeforeReset == liveRegions.load(std::memory_order_relaxed),
           "Expected no new regions allocation.");
    ok = pool_destroy(pool);
    REQUIRE(ok);
    REQUIRE_MESSAGE(!liveRegions.load(std::memory_order_relaxed), "Expected all regions were released.");
}

class SharedPoolRun: utils::NoAssign {
    static long                 threadNum;
    static utils::SpinBarrier startB,
                                mallocDone;
    static rml::MemoryPool     *pool;
    static void               **crossThread,
                              **afterTerm;
public:
    static const int OBJ_CNT = 100;

    static void init(int num, rml::MemoryPool *pl, void **crThread, void **aTerm) {
        threadNum = num;
        pool = pl;
        crossThread = crThread;
        afterTerm = aTerm;
        startB.initialize(threadNum);
        mallocDone.initialize(threadNum);
    }

    void operator()( int id ) const {
        const int ITERS = 1000;
        void *local[ITERS];

        startB.wait();
        for (int i=id*OBJ_CNT; i<(id+1)*OBJ_CNT; i++) {
            afterTerm[i] = pool_malloc(pool, i%2? 8*1024 : 9*1024);
            memset(afterTerm[i], i, i%2? 8*1024 : 9*1024);
            crossThread[i] = pool_malloc(pool, i%2? 9*1024 : 8*1024);
            memset(crossThread[i], i, i%2? 9*1024 : 8*1024);
        }

        for (int i=1; i<ITERS; i+=2) {
            local[i-1] = pool_malloc(pool, 6*1024);
            memset(local[i-1], i, 6*1024);
            local[i] = pool_malloc(pool, 16*1024);
            memset(local[i], i, 16*1024);
        }
        mallocDone.wait();
        int myVictim = threadNum-id-1;
        for (int i=myVictim*OBJ_CNT; i<(myVictim+1)*OBJ_CNT; i++)
            pool_free(pool, crossThread[i]);
        for (int i=0; i<ITERS; i++)
            pool_free(pool, local[i]);
    }
};

long                 SharedPoolRun::threadNum;
utils::SpinBarrier SharedPoolRun::startB,
                     SharedPoolRun::mallocDone;
rml::MemoryPool     *SharedPoolRun::pool;
void               **SharedPoolRun::crossThread,
                   **SharedPoolRun::afterTerm;

// single pool shared by different threads
void TestSharedPool()
{
    rml::MemPoolPolicy pol(getMallocMem, putMallocMem);
    rml::MemoryPool *pool;

    pool_create_v1(0, &pol, &pool);
    void **crossThread = new void*[utils::MaxThread * SharedPoolRun::OBJ_CNT];
    void **afterTerm = new void*[utils::MaxThread * SharedPoolRun::OBJ_CNT];

    for (int p=utils::MinThread; p<=utils::MaxThread; p++) {
        SharedPoolRun::init(p, pool, crossThread, afterTerm);
        SharedPoolRun thr;

        void *hugeObj = pool_malloc(pool, 10*1024*1024);
        REQUIRE(hugeObj);

        utils::NativeParallelFor( p, thr );

        pool_free(pool, hugeObj);
        for (int i=0; i<p*SharedPoolRun::OBJ_CNT; i++)
            pool_free(pool, afterTerm[i]);
    }
    delete []afterTerm;
    delete []crossThread;

    bool ok = pool_destroy(pool);
    REQUIRE(ok);
    REQUIRE_MESSAGE(!liveRegions.load(std::memory_order_relaxed), "Expected all regions were released.");
}

void *CrossThreadGetMem(intptr_t pool_id, size_t &bytes)
{
    if (poolSpace[pool_id].pos + bytes > poolSpace[pool_id].bufSize)
        return nullptr;

    void *ret = poolSpace[pool_id].space + poolSpace[pool_id].pos;
    poolSpace[pool_id].pos += bytes;
    poolSpace[pool_id].regions++;

    return ret;
}

int CrossThreadPutMem(intptr_t pool_id, void* /*raw_ptr*/, size_t /*raw_bytes*/)
{
    poolSpace[pool_id].regions--;
    return 0;
}

class CrossThreadRun: utils::NoAssign {
    static long number_of_threads;
    static utils::SpinBarrier barrier;
    static rml::MemoryPool **pool;
    static char **obj;
public:
    static void initBarrier(unsigned thrds) { barrier.initialize(thrds); }
    static void init(long num) {
        number_of_threads = num;
        pool = new rml::MemoryPool*[number_of_threads];
        poolSpace = new PoolSpace[number_of_threads];
        obj = new char*[number_of_threads];
    }
    static void destroy() {
        for (long i=0; i<number_of_threads; i++)
            REQUIRE_MESSAGE(!poolSpace[i].regions, "Memory leak detected");
        delete []pool;
        delete []poolSpace;
        delete []obj;
    }
    CrossThreadRun() {}
    void operator()( int id ) const {
        rml::MemPoolPolicy pol(CrossThreadGetMem, CrossThreadPutMem);
        const int objLen = 10*id;

        pool_create_v1(id, &pol, &pool[id]);
        obj[id] = (char*)pool_malloc(pool[id], objLen);
        REQUIRE(obj[id]);
        memset(obj[id], id, objLen);

        {
            const size_t lrgSz = 2*16*1024;
            void *ptrLarge = pool_malloc(pool[id], lrgSz);
            REQUIRE(ptrLarge);
            memset(ptrLarge, 1, lrgSz);
            // consume all small objects
            while (pool_malloc(pool[id], 5 * 1024));
            // releasing of large object will not give a chance to allocate more
            // since only fixed pool can look at other bins aligned/notAligned
            pool_free(pool[id], ptrLarge);
            CHECK(!pool_malloc(pool[id], 5*1024));
        }

        barrier.wait();
        int myPool = number_of_threads-id-1;
        for (int i=0; i<10*myPool; i++)
            REQUIRE(myPool==obj[myPool][i]);
        pool_free(pool[myPool], obj[myPool]);
        bool ok = pool_destroy(pool[myPool]);
        REQUIRE(ok);
    }
};

long CrossThreadRun::number_of_threads;
utils::SpinBarrier CrossThreadRun::barrier;
rml::MemoryPool **CrossThreadRun::pool;
char **CrossThreadRun::obj;

// pools created, used and destroyed by different threads
void TestCrossThreadPools()
{
    for (int p=utils::MinThread; p<=utils::MaxThread; p++) {
        CrossThreadRun::initBarrier(p);
        CrossThreadRun::init(p);
        utils::NativeParallelFor( p, CrossThreadRun() );
        for (int i=0; i<p; i++)
            REQUIRE_MESSAGE(!poolSpace[i].regions, "Region leak detected");
        CrossThreadRun::destroy();
    }
}

// buffer is too small to pool be created, but must not leak resources
void TestTooSmallBuffer()
{
    poolSpace = new PoolSpace(8*1024);

    rml::MemPoolPolicy pol(CrossThreadGetMem, CrossThreadPutMem);
    rml::MemoryPool *pool;
    pool_create_v1(0, &pol, &pool);
    bool ok = pool_destroy(pool);
    REQUIRE(ok);
    REQUIRE_MESSAGE(!poolSpace[0].regions, "No leaks.");

    delete poolSpace;
}

class FixedPoolHeadBase : utils::NoAssign {
    size_t size;
    std::atomic<bool> used;
    char* data;
public:
    FixedPoolHeadBase(size_t s) : size(s), used(false) {
        data = new char[size];
    }
    void *useData(size_t &bytes) {
        bool wasUsed = used.exchange(true);
        REQUIRE_MESSAGE(!wasUsed, "The buffer must not be used twice.");
        bytes = size;
        return data;
    }
    ~FixedPoolHeadBase() {
        delete []data;
    }
};

template<size_t SIZE>
class FixedPoolHead : FixedPoolHeadBase {
public:
    FixedPoolHead() : FixedPoolHeadBase(SIZE) { }
};

static void *fixedBufGetMem(intptr_t pool_id, size_t &bytes)
{
    return ((FixedPoolHeadBase*)pool_id)->useData(bytes);
}

class FixedPoolUse: utils::NoAssign {
    static utils::SpinBarrier startB;
    rml::MemoryPool *pool;
    size_t reqSize;
    int iters;
public:
    FixedPoolUse(unsigned threads, rml::MemoryPool *p, size_t sz, int it) :
        pool(p), reqSize(sz), iters(it) {
        startB.initialize(threads);
    }
    void operator()( int /*id*/ ) const {
        startB.wait();
        for (int i=0; i<iters; i++) {
            void *o = pool_malloc(pool, reqSize);
            ASSERT(o, "Invalid object");
            pool_free(pool, o);
        }
    }
};

utils::SpinBarrier FixedPoolUse::startB;

class FixedPoolNomem: utils::NoAssign {
    utils::SpinBarrier *startB;
    rml::MemoryPool *pool;
public:
    FixedPoolNomem(utils::SpinBarrier *b, rml::MemoryPool *p) :
        startB(b), pool(p) {}
    void operator()(int id) const {
        startB->wait();
        void *o = pool_malloc(pool, id%2? 64 : 128*1024);
        ASSERT(!o, "All memory must be consumed.");
    }
};

class FixedPoolSomeMem: utils::NoAssign {
    utils::SpinBarrier *barrier;
    rml::MemoryPool *pool;
public:
    FixedPoolSomeMem(utils::SpinBarrier *b, rml::MemoryPool *p) :
        barrier(b), pool(p) {}
    void operator()(int id) const {
        barrier->wait();
        utils::Sleep(2*id);
        void *o = pool_malloc(pool, id%2? 64 : 128*1024);
        barrier->wait();
        pool_free(pool, o);
    }
};

bool haveEnoughSpace(rml::MemoryPool *pool, size_t sz)
{
    if (void *p = pool_malloc(pool, sz)) {
        pool_free(pool, p);
        return true;
    }
    return false;
}

void TestFixedBufferPool()
{
    const int ITERS = 7;
    const size_t MAX_OBJECT = 7*1024*1024;
    void *ptrs[ITERS];
    rml::MemPoolPolicy pol(fixedBufGetMem, nullptr, 0, /*fixedSizePool=*/true,
                           /*keepMemTillDestroy=*/false);
    rml::MemoryPool *pool;
    {
        FixedPoolHead<MAX_OBJECT + 1024*1024> head;

        pool_create_v1((intptr_t)&head, &pol, &pool);
        {
            utils::NativeParallelFor( 1, FixedPoolUse(1, pool, MAX_OBJECT, 2) );

            for (int i=0; i<ITERS; i++) {
                ptrs[i] = pool_malloc(pool, MAX_OBJECT/ITERS);
                REQUIRE(ptrs[i]);
            }
            for (int i=0; i<ITERS; i++)
                pool_free(pool, ptrs[i]);

            utils::NativeParallelFor( 1, FixedPoolUse(1, pool, MAX_OBJECT, 1) );
        }
        // each thread asks for an MAX_OBJECT/p/2 object,
        // /2 is to cover fragmentation
        for (int p=utils::MinThread; p<=utils::MaxThread; p++) {
            utils::NativeParallelFor( p, FixedPoolUse(p, pool, MAX_OBJECT/p/2, 10000) );
        }
        {
            const int p = 128;
            utils::NativeParallelFor( p, FixedPoolUse(p, pool, MAX_OBJECT/p/2, 1) );
        }
        {
            size_t maxSz;
            const int p = 256;
            utils::SpinBarrier barrier(p);

            // Find maximal useful object size. Start with MAX_OBJECT/2,
            // as the pool might be fragmented by BootStrapBlocks consumed during
            // FixedPoolRun.
            size_t l, r;
            REQUIRE(haveEnoughSpace(pool, MAX_OBJECT/2));
            for (l = MAX_OBJECT/2, r = MAX_OBJECT + 1024*1024; l < r-1; ) {
                size_t mid = (l+r)/2;
                if (haveEnoughSpace(pool, mid))
                    l = mid;
                else
                    r = mid;
            }
            maxSz = l;
            REQUIRE_MESSAGE(!haveEnoughSpace(pool, maxSz+1), "Expect to find boundary value.");
            // consume all available memory
            void *largeObj = pool_malloc(pool, maxSz);
            REQUIRE(largeObj);
            void *o = pool_malloc(pool, 64);
            if (o) // pool fragmented, skip FixedPoolNomem
                pool_free(pool, o);
            else
                utils::NativeParallelFor( p, FixedPoolNomem(&barrier, pool) );
            pool_free(pool, largeObj);
            // keep some space unoccupied
            largeObj = pool_malloc(pool, maxSz-512*1024);
            REQUIRE(largeObj);
            utils::NativeParallelFor( p, FixedPoolSomeMem(&barrier, pool) );
            pool_free(pool, largeObj);
        }
        bool ok = pool_destroy(pool);
        REQUIRE(ok);
    }
    // check that fresh untouched pool can successfully fulfil requests from 128 threads
    {
        FixedPoolHead<MAX_OBJECT + 1024*1024> head;
        pool_create_v1((intptr_t)&head, &pol, &pool);
        int p=128;
        utils::NativeParallelFor( p, FixedPoolUse(p, pool, MAX_OBJECT/p/2, 1) );
        bool ok = pool_destroy(pool);
        REQUIRE(ok);
    }
}

static size_t currGranularity;

static void *getGranMem(intptr_t /*pool_id*/, size_t &bytes)
{
    REQUIRE_MESSAGE(!(bytes%currGranularity), "Region size mismatch granularity.");
    return malloc(bytes);
}

static int putGranMem(intptr_t /*pool_id*/, void *ptr, size_t bytes)
{
    REQUIRE_MESSAGE(!(bytes%currGranularity), "Region size mismatch granularity.");
    free(ptr);
    return 0;
}

void TestPoolGranularity()
{
    rml::MemPoolPolicy pol(getGranMem, putGranMem);
    const size_t grans[] = {4*1024, 2*1024*1024, 6*1024*1024, 10*1024*1024};

    for (unsigned i=0; i<sizeof(grans)/sizeof(grans[0]); i++) {
        pol.granularity = currGranularity = grans[i];
        rml::MemoryPool *pool;

        pool_create_v1(0, &pol, &pool);
        for (int sz=500*1024; sz<16*1024*1024; sz+=101*1024) {
            void *p = pool_malloc(pool, sz);
            REQUIRE_MESSAGE(p, "Can't allocate memory in pool.");
            pool_free(pool, p);
        }
        bool ok = pool_destroy(pool);
        REQUIRE(ok);
    }
}

static size_t putMemAll, getMemAll, getMemSuccessful;

static void *getMemMalloc(intptr_t /*pool_id*/, size_t &bytes)
{
    getMemAll++;
    void *p = malloc(bytes);
    if (p)
        getMemSuccessful++;
    return p;
}

static int putMemFree(intptr_t /*pool_id*/, void *ptr, size_t /*bytes*/)
{
    putMemAll++;
    free(ptr);
    return 0;
}

void TestPoolKeepTillDestroy()
{
    const int ITERS = 50*1024;
    void *ptrs[2*ITERS+1];
    rml::MemPoolPolicy pol(getMemMalloc, putMemFree);
    rml::MemoryPool *pool;

    // 1st create default pool that returns memory back to callback,
    // then use keepMemTillDestroy policy
    for (int keep=0; keep<2; keep++) {
        getMemAll = putMemAll = 0;
        if (keep)
            pol.keepAllMemory = 1;
        pool_create_v1(0, &pol, &pool);
        for (int i=0; i<2*ITERS; i+=2) {
            ptrs[i] = pool_malloc(pool, 7*1024);
            ptrs[i+1] = pool_malloc(pool, 10*1024);
        }
        ptrs[2*ITERS] = pool_malloc(pool, 8*1024*1024);
        REQUIRE(!putMemAll);
        for (int i=0; i<2*ITERS; i++)
            pool_free(pool, ptrs[i]);
        pool_free(pool, ptrs[2*ITERS]);
        size_t totalPutMemCalls = putMemAll;
        if (keep)
            REQUIRE(!putMemAll);
        else {
            REQUIRE(putMemAll);
            putMemAll = 0;
        }
        size_t getCallsBefore = getMemAll;
        void *p = pool_malloc(pool, 8*1024*1024);
        REQUIRE(p);
        if (keep)
            REQUIRE_MESSAGE(getCallsBefore == getMemAll, "Must not lead to new getMem call");
        size_t putCallsBefore = putMemAll;
        bool ok = pool_reset(pool);
        REQUIRE(ok);
        REQUIRE_MESSAGE(putCallsBefore == putMemAll, "Pool is not releasing memory during reset.");
        ok = pool_destroy(pool);
        REQUIRE(ok);
        REQUIRE(putMemAll);
        totalPutMemCalls += putMemAll;
        REQUIRE_MESSAGE(getMemAll == totalPutMemCalls, "Memory leak detected.");
    }

}

static bool memEqual(char *buf, size_t size, int val)
{
    bool memEq = true;
    for (size_t k=0; k<size; k++)
        if (buf[k] != val)
             memEq = false;
    return memEq;
}

void TestEntries()
{
    const int SZ = 4;
    const int ALGN = 4;
    size_t size[SZ] = {8, 8000, 9000, 100*1024};
    size_t algn[ALGN] = {8, 64, 4*1024, 8*1024*1024};

    rml::MemPoolPolicy pol(getGranMem, putGranMem);
    currGranularity = 1; // not check granularity in the test
    rml::MemoryPool *pool;

    pool_create_v1(0, &pol, &pool);
    for (int i=0; i<SZ; i++)
        for (int j=0; j<ALGN; j++) {
            char *p = (char*)pool_aligned_malloc(pool, size[i], algn[j]);
            REQUIRE((p && 0==((uintptr_t)p & (algn[j]-1))));
            memset(p, j, size[i]);

            size_t curr_algn = algn[rand() % ALGN];
            size_t curr_sz = size[rand() % SZ];
            char *p1 = (char*)pool_aligned_realloc(pool, p, curr_sz, curr_algn);
            REQUIRE((p1 && 0==((uintptr_t)p1 & (curr_algn-1))));
            REQUIRE(memEqual(p1, utils::min(size[i], curr_sz), j));

            memset(p1, j+1, curr_sz);
            size_t curr_sz1 = size[rand() % SZ];
            char *p2 = (char*)pool_realloc(pool, p1, curr_sz1);
            REQUIRE(p2);
            REQUIRE(memEqual(p2, utils::min(curr_sz1, curr_sz), j+1));

            pool_free(pool, p2);
        }

    bool ok = pool_destroy(pool);
    REQUIRE(ok);

    bool fail = rml::pool_destroy(nullptr);
    REQUIRE(!fail);
    fail = rml::pool_reset(nullptr);
    REQUIRE(!fail);
}

rml::MemoryPool *CreateUsablePool(size_t size)
{
    rml::MemoryPool *pool;
    rml::MemPoolPolicy okPolicy(getMemMalloc, putMemFree);

    putMemAll = getMemAll = getMemSuccessful = 0;
    rml::MemPoolError res = pool_create_v1(0, &okPolicy, &pool);
    if (res != rml::POOL_OK) {
        REQUIRE_MESSAGE((!getMemAll && !putMemAll), "No callbacks after fail.");
        return nullptr;
    }
    void *o = pool_malloc(pool, size);
    if (!getMemSuccessful) {
        // no memory from callback, valid reason to leave
        REQUIRE_MESSAGE(!o, "The pool must be unusable.");
        return nullptr;
    }
    REQUIRE_MESSAGE(o, "Created pool must be useful.");
    REQUIRE_MESSAGE((getMemSuccessful == 1 || getMemSuccessful == 5 || getMemAll > getMemSuccessful),
           "Multiple requests are allowed when unsuccessful request occurred or cannot search in bootstrap memory. ");
    REQUIRE(!putMemAll);
    pool_free(pool, o);

    return pool;
}

void CheckPoolLeaks(size_t poolsAlwaysAvailable)
{
    const size_t MAX_POOLS = 16*1000;
    const int ITERS = 20, CREATED_STABLE = 3;
    rml::MemoryPool *pools[MAX_POOLS];
    size_t created, maxCreated = MAX_POOLS;
    int maxNotChangedCnt = 0;

    // expecting that for ITERS runs, max number of pools that can be created
    // can be stabilized and still stable CREATED_STABLE times
    for (int j=0; j<ITERS && maxNotChangedCnt<CREATED_STABLE; j++) {
        for (created=0; created<maxCreated; created++) {
            rml::MemoryPool *p = CreateUsablePool(1024);
            if (!p)
                break;
            pools[created] = p;
        }
        REQUIRE_MESSAGE(created>=poolsAlwaysAvailable,
               "Expect that the reasonable number of pools can be always created.");
        for (size_t i=0; i<created; i++) {
            bool ok = pool_destroy(pools[i]);
            REQUIRE(ok);
        }
        if (created < maxCreated) {
            maxCreated = created;
            maxNotChangedCnt = 0;
        } else
            maxNotChangedCnt++;
    }
    REQUIRE_MESSAGE(maxNotChangedCnt == CREATED_STABLE, "The number of created pools must be stabilized.");
}

void TestPoolCreation()
{
    putMemAll = getMemAll = getMemSuccessful = 0;

    rml::MemPoolPolicy nullPolicy(nullptr, putMemFree),
        emptyFreePolicy(getMemMalloc, nullptr),
        okPolicy(getMemMalloc, putMemFree);
    rml::MemoryPool *pool;

    rml::MemPoolError res = pool_create_v1(0, &nullPolicy, &pool);
    REQUIRE_MESSAGE(res==rml::INVALID_POLICY, "pool with empty pAlloc can't be created");
    res = pool_create_v1(0, &emptyFreePolicy, &pool);
    REQUIRE_MESSAGE(res==rml::INVALID_POLICY, "pool with empty pFree can't be created");
    REQUIRE_MESSAGE((!putMemAll && !getMemAll), "no callback calls are expected");
    res = pool_create_v1(0, &okPolicy, &pool);
    REQUIRE(res==rml::POOL_OK);
    bool ok = pool_destroy(pool);
    REQUIRE(ok);
    REQUIRE_MESSAGE(putMemAll == getMemSuccessful, "no leaks after pool_destroy");

    // 32 is a guess for a number of pools that is acceptable everywere
    CheckPoolLeaks(32);
    // try to consume all but 16 TLS keys
    LimitTLSKeysTo limitTLSTo(16);
    // ...and check that we can create at least 16 pools
    CheckPoolLeaks(16);
}

struct AllocatedObject {
    rml::MemoryPool *pool;
};

const size_t BUF_SIZE = 1024*1024;

class PoolIdentityCheck : utils::NoAssign {
    rml::MemoryPool** const pools;
    AllocatedObject** const objs;
public:
    PoolIdentityCheck(rml::MemoryPool** p, AllocatedObject** o) : pools(p), objs(o) {}
    void operator()(int id) const {
        objs[id] = (AllocatedObject*)pool_malloc(pools[id], BUF_SIZE/2);
        REQUIRE(objs[id]);
        rml::MemoryPool *act_pool = rml::pool_identify(objs[id]);
        REQUIRE(act_pool == pools[id]);

        for (size_t total=0; total<2*BUF_SIZE; total+=256) {
            AllocatedObject *o = (AllocatedObject*)pool_malloc(pools[id], 256);
            REQUIRE(o);
            act_pool = rml::pool_identify(o);
            REQUIRE(act_pool == pools[id]);
            pool_free(act_pool, o);
        }
        if( id&1 ) { // make every second returned object "small"
            pool_free(act_pool, objs[id]);
            objs[id] = (AllocatedObject*)pool_malloc(pools[id], 16);
            REQUIRE(objs[id]);
        }
        objs[id]->pool = act_pool;
    }
};

void TestPoolDetection()
{
    const int POOLS = 4;
    rml::MemPoolPolicy pol(fixedBufGetMem, nullptr, 0, /*fixedSizePool=*/true,
                           /*keepMemTillDestroy=*/false);
    rml::MemoryPool *pools[POOLS];
    FixedPoolHead<BUF_SIZE*POOLS> head[POOLS];
    AllocatedObject *objs[POOLS];

    for (int i=0; i<POOLS; i++)
        pool_create_v1((intptr_t)(head+i), &pol, &pools[i]);
    // if object somehow released to different pools, subsequent allocation
    // from affected pools became impossible
    for (int k=0; k<10; k++) {
        PoolIdentityCheck check(pools, objs);
        if( k&1 )
            utils::NativeParallelFor( POOLS, check);
        else
            for (int i=0; i<POOLS; i++) check(i);

        for (int i=0; i<POOLS; i++) {
            rml::MemoryPool *p = rml::pool_identify(objs[i]);
            REQUIRE(p == objs[i]->pool);
            pool_free(p, objs[i]);
        }
    }
    for (int i=0; i<POOLS; i++) {
        bool ok = pool_destroy(pools[i]);
        REQUIRE(ok);
    }
}

void TestLazyBootstrap()
{
    rml::MemPoolPolicy pol(getMemMalloc, putMemFree);
    const size_t sizes[] = {8, 9*1024, 0};

    for (int i=0; sizes[i]; i++) {
        rml::MemoryPool *pool = CreateUsablePool(sizes[i]);
        bool ok = pool_destroy(pool);
        REQUIRE(ok);
        REQUIRE_MESSAGE(getMemSuccessful == putMemAll, "No leak.");
    }
}

class NoLeakOnDestroyRun: utils::NoAssign {
    rml::MemoryPool      *pool;
    utils::SpinBarrier *barrier;
public:
    NoLeakOnDestroyRun(rml::MemoryPool *p, utils::SpinBarrier *b) : pool(p), barrier(b) {}
    void operator()(int id) const {
        void *p = pool_malloc(pool, id%2? 8 : 9000);
        REQUIRE((p && liveRegions.load(std::memory_order_relaxed)));
        barrier->wait();
        if (!id) {
            bool ok = pool_destroy(pool);
            REQUIRE(ok);
            REQUIRE_MESSAGE(!liveRegions.load(std::memory_order_relaxed), "Expected all regions were released.");
        }
        // other threads must wait till pool destruction,
        // to not call thread destruction cleanup before this
        barrier->wait();
    }
};

void TestNoLeakOnDestroy()
{
    liveRegions.store(0, std::memory_order_release);
    for (int p=utils::MinThread; p<=utils::MaxThread; p++) {
        rml::MemPoolPolicy pol(getMallocMem, putMallocMem);
        utils::SpinBarrier barrier(p);
        rml::MemoryPool *pool;

        pool_create_v1(0, &pol, &pool);
        utils::NativeParallelFor(p, NoLeakOnDestroyRun(pool, &barrier));
    }
}

static int putMallocMemError(intptr_t /*pool_id*/, void *ptr, size_t bytes)
{
    MallocPoolHeader *hdr = (MallocPoolHeader*)ptr-1;
    REQUIRE_MESSAGE(bytes == hdr->userSize, "Invalid size in pool callback.");
    free(hdr->rawPtr);

    liveRegions--;

    return -1;
}

void TestDestroyFailed()
{
    rml::MemPoolPolicy pol(getMallocMem, putMallocMemError);
    rml::MemoryPool *pool;
    pool_create_v1(0, &pol, &pool);
    void *ptr = pool_malloc(pool, 16);
    REQUIRE(ptr);
    bool fail = pool_destroy(pool);
    REQUIRE_MESSAGE(fail==false, "putMemPolicyError callback returns error, "
           "expect pool_destroy() failure");
}

void TestPoolMSize() {
    rml::MemoryPool *pool = CreateUsablePool(1024);

    const int SZ = 10;
    // Original allocation requests, random numbers from small to large
    size_t requestedSz[SZ] = {8, 16, 500, 1000, 2000, 4000, 8000, 1024*1024, 4242+4242, 8484+8484};

    // Unlike large objects, small objects do not store its original size along with the object itself
    // On Power architecture TLS bins are divided differently.
    size_t allocatedSz[SZ] =
#if __powerpc64__ || __ppc64__ || __bgp__
        {8, 16, 512, 1024, 2688, 5376, 8064, 1024*1024, 4242+4242, 8484+8484};
#else
        {8, 16, 512, 1024, 2688, 4032, 8128, 1024*1024, 4242+4242, 8484+8484};
#endif
    for (int i = 0; i < SZ; i++) {
        void* obj = pool_malloc(pool, requestedSz[i]);
        size_t objSize = pool_msize(pool, obj);
        REQUIRE_MESSAGE(objSize == allocatedSz[i], "pool_msize returned the wrong value");
        pool_free(pool, obj);
    }
    bool destroyed = pool_destroy(pool);
    REQUIRE(destroyed);
}

//! \brief \ref error_guessing
TEST_CASE("Too small buffer") {
    TestTooSmallBuffer();
}

//! \brief \ref error_guessing
TEST_CASE("Pool reset") {
    TestPoolReset();
}
TEST_CASE("Shared pool") {
    TestSharedPool();
}

//! \brief \ref error_guessing
TEST_CASE("Cross thread pools") {
    TestCrossThreadPools();
}

//! \brief \ref interface
TEST_CASE("Fixed buffer pool") {
    TestFixedBufferPool();
}

//! \brief \ref interface
TEST_CASE("Pool granularity") {
    TestPoolGranularity();
}

//! \brief \ref error_guessing
TEST_CASE("Keep pool till destroy") {
    TestPoolKeepTillDestroy();
}

//! \brief \ref error_guessing
TEST_CASE("Entries") {
    TestEntries();
}

//! \brief \ref interface
TEST_CASE("Pool creation") {
    TestPoolCreation();
}

//! \brief \ref error_guessing
TEST_CASE("Pool detection") {
    TestPoolDetection();
}

//! \brief \ref error_guessing
TEST_CASE("Lazy bootstrap") {
    TestLazyBootstrap();
}

//! \brief \ref error_guessing
TEST_CASE("No leak on destroy") {
    TestNoLeakOnDestroy();
}

//! \brief \ref error_guessing
TEST_CASE("Destroy failed") {
    TestDestroyFailed();
}

//! \brief \ref interface
TEST_CASE("Pool msize") {
    TestPoolMSize();
}
