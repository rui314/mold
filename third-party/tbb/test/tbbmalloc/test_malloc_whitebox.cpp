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

//! \file test_malloc_whitebox.cpp
//! \brief Test for [memory_allocation] functionality

#if _WIN32 || _WIN64
#define _CRT_SECURE_NO_WARNINGS
#endif

// To prevent loading dynamic TBBmalloc at startup, that is not needed for the whitebox test
#define __TBB_SOURCE_DIRECTLY_INCLUDED 1
// Call thread shutdown API for native threads join
#define HARNESS_TBBMALLOC_THREAD_SHUTDOWN 1

// According to C99 standard INTPTR_MIN defined for C++ if __STDC_LIMIT_MACROS pre-defined
#define __STDC_LIMIT_MACROS 1

// To not depends on ITT support stuff
#ifdef DO_ITT_NOTIFY
#undef DO_ITT_NOTIFY
#endif

#include "common/test.h"

#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/utils_env.h"
#include "common/spin_barrier.h"

#include "oneapi/tbb/detail/_machine.h"

#define __TBB_MALLOC_WHITEBOX_TEST 1 // to get access to allocator internals
// help trigger rare race condition
#define WhiteboxTestingYield() (tbb::detail::yield(), tbb::detail::yield(), tbb::detail::yield(), tbb::detail::yield())

#if __INTEL_COMPILER && __TBB_MIC_OFFLOAD
// 2571 is variable has not been declared with compatible "target" attribute
// 3218 is class/struct may fail when offloaded because this field is misaligned
//         or contains data that is misaligned
    #pragma warning(push)
    #pragma warning(disable:2571 3218)
#endif
#define protected public
#define private public
#include "../../src/tbbmalloc/frontend.cpp"
#undef protected
#undef private
#if __INTEL_COMPILER && __TBB_MIC_OFFLOAD
    #pragma warning(pop)
#endif
#include "../../src/tbbmalloc/backend.cpp"
#include "../../src/tbbmalloc/backref.cpp"

namespace tbbmalloc_whitebox {
    std::atomic<size_t> locGetProcessed{};
    std::atomic<size_t> locPutProcessed{};
}
#include "../../src/tbbmalloc/large_objects.cpp"
#include "../../src/tbbmalloc/tbbmalloc.cpp"

const int LARGE_MEM_SIZES_NUM = 10;
static const int MinThread = 1;
static const int MaxThread = 4;

class AllocInfo {
    int *p;
    int val;
    int size;
public:
    AllocInfo() : p(nullptr), val(0), size(0) {}
    explicit AllocInfo(int sz) : p((int*)scalable_malloc(sz*sizeof(int))),
                                   val(rand()), size(sz) {
        REQUIRE(p);
        for (int k=0; k<size; k++)
            p[k] = val;
    }
    void check() const {
        for (int k=0; k<size; k++)
            ASSERT(p[k] == val, nullptr);
    }
    void clear() {
        scalable_free(p);
    }
};

// Test struct to call ProcessShutdown after all tests
struct ShutdownTest {
    ~ShutdownTest() {
    #if _WIN32 || _WIN64
        __TBB_mallocProcessShutdownNotification(true);
    #else
        __TBB_mallocProcessShutdownNotification(false);
    #endif
    }
};

static ShutdownTest shutdownTest;

class SimpleBarrier: utils::NoAssign {
protected:
    static utils::SpinBarrier barrier;
public:
    static void initBarrier(unsigned thrds) { barrier.initialize(thrds); }
};

utils::SpinBarrier SimpleBarrier::barrier;

class TestLargeObjCache: public SimpleBarrier {
public:
    static int largeMemSizes[LARGE_MEM_SIZES_NUM];

    TestLargeObjCache( ) {}

    void operator()( int /*mynum*/ ) const {
        AllocInfo allocs[LARGE_MEM_SIZES_NUM];

        // push to maximal cache limit
        for (int i=0; i<2; i++) {
            const int sizes[] = { MByte/sizeof(int),
                                  (MByte-2*LargeObjectCache::LargeBSProps::CacheStep)/sizeof(int) };
            for (int q=0; q<2; q++) {
                size_t curr = 0;
                for (int j=0; j<LARGE_MEM_SIZES_NUM; j++, curr++)
                    new (allocs+curr) AllocInfo(sizes[q]);

                for (size_t j=0; j<curr; j++) {
                    allocs[j].check();
                    allocs[j].clear();
                }
            }
        }

        barrier.wait();

        // check caching correctness
        for (int i=0; i<1000; i++) {
            size_t curr = 0;
            for (int j=0; j<LARGE_MEM_SIZES_NUM-1; j++, curr++)
                new (allocs+curr) AllocInfo(largeMemSizes[j]);

            new (allocs+curr)
                AllocInfo((int)(4*minLargeObjectSize +
                                2*minLargeObjectSize*(1.*rand()/RAND_MAX)));
            curr++;

            for (size_t j=0; j<curr; j++) {
                allocs[j].check();
                allocs[j].clear();
            }
        }
    }
};

int TestLargeObjCache::largeMemSizes[LARGE_MEM_SIZES_NUM];

void TestLargeObjectCache()
{
    for (int i=0; i<LARGE_MEM_SIZES_NUM; i++)
        TestLargeObjCache::largeMemSizes[i] =
            (int)(minLargeObjectSize + 2*minLargeObjectSize*(1.*rand()/RAND_MAX));

    for( int p=MaxThread; p>=MinThread; --p ) {
        TestLargeObjCache::initBarrier( p );
        utils::NativeParallelFor( p, TestLargeObjCache() );
    }
}

#if MALLOC_CHECK_RECURSION

class TestStartupAlloc: public SimpleBarrier {
    struct TestBlock {
        void *ptr;
        size_t sz;
    };
    static const int ITERS = 100;
public:
    TestStartupAlloc() {}
    void operator()(int) const {
        TestBlock blocks1[ITERS], blocks2[ITERS];

        barrier.wait();

        for (int i=0; i<ITERS; i++) {
            blocks1[i].sz = rand() % minLargeObjectSize;
            blocks1[i].ptr = StartupBlock::allocate(blocks1[i].sz);
            REQUIRE((blocks1[i].ptr && StartupBlock::msize(blocks1[i].ptr)>=blocks1[i].sz
                   && 0==(uintptr_t)blocks1[i].ptr % sizeof(void*)));
            memset(blocks1[i].ptr, i, blocks1[i].sz);
        }
        for (int i=0; i<ITERS; i++) {
            blocks2[i].sz = rand() % minLargeObjectSize;
            blocks2[i].ptr = StartupBlock::allocate(blocks2[i].sz);
            REQUIRE((blocks2[i].ptr && StartupBlock::msize(blocks2[i].ptr)>=blocks2[i].sz
                   && 0==(uintptr_t)blocks2[i].ptr % sizeof(void*)));
            memset(blocks2[i].ptr, i, blocks2[i].sz);

            for (size_t j=0; j<blocks1[i].sz; j++)
                REQUIRE(*((char*)blocks1[i].ptr+j) == i);
            Block *block = (Block *)alignDown(blocks1[i].ptr, slabSize);
            ((StartupBlock *)block)->free(blocks1[i].ptr);
        }
        for (int i=ITERS-1; i>=0; i--) {
            for (size_t j=0; j<blocks2[i].sz; j++)
                REQUIRE(*((char*)blocks2[i].ptr+j) == i);
            Block *block = (Block *)alignDown(blocks2[i].ptr, slabSize);
            ((StartupBlock *)block)->free(blocks2[i].ptr);
        }
    }
};

#endif /* MALLOC_CHECK_RECURSION */

#include <deque>

template<int ITERS>
class BackRefWork: utils::NoAssign {
    struct TestBlock {
        BackRefIdx idx;
        char       data;
        TestBlock(BackRefIdx idx_) : idx(idx_) {}
    };
public:
    BackRefWork() {}
    void operator()(int) const {
        size_t cnt;
        // it's important to not invalidate pointers to the contents of the container
        std::deque<TestBlock> blocks;

        // for ITERS==0 consume all available backrefs
        for (cnt=0; !ITERS || cnt<ITERS; cnt++) {
            BackRefIdx idx = BackRefIdx::newBackRef(/*largeObj=*/false);
            if (idx.isInvalid())
                break;
            blocks.push_back(TestBlock(idx));
            setBackRef(blocks.back().idx, &blocks.back().data);
        }
        for (size_t i=0; i<cnt; i++)
            REQUIRE((Block*)&blocks[i].data == getBackRef(blocks[i].idx));
        for (size_t i=cnt; i>0; i--)
            removeBackRef(blocks[i-1].idx);
    }
};

class LocalCachesHit: utils::NoAssign {
    // set ITERS to trigger possible leak of backreferences
    // during cleanup on cache overflow and on thread termination
    static const int ITERS = 2*(FreeBlockPool::POOL_HIGH_MARK +
                                LocalLOC::LOC_HIGH_MARK);
public:
    LocalCachesHit() {}
    void operator()(int) const {
        void *objsSmall[ITERS], *objsLarge[ITERS];

        for (int i=0; i<ITERS; i++) {
            objsSmall[i] = scalable_malloc(minLargeObjectSize-1);
            objsLarge[i] = scalable_malloc(minLargeObjectSize);
        }
        for (int i=0; i<ITERS; i++) {
            scalable_free(objsSmall[i]);
            scalable_free(objsLarge[i]);
        }
    }
};

static size_t allocatedBackRefCount()
{
    size_t cnt = 0;
    for (int i=0; i<=backRefMain.load(std::memory_order_relaxed)->lastUsed.load(std::memory_order_relaxed); i++)
        cnt += backRefMain.load(std::memory_order_relaxed)->backRefBl[i]->allocatedCount;
    return cnt;
}

class TestInvalidBackrefs: public SimpleBarrier {
#if __ANDROID__
    // Android requires lower iters due to lack of virtual memory.
    static const int BACKREF_GROWTH_ITERS = 50*1024;
#else
    static const int BACKREF_GROWTH_ITERS = 200*1024;
#endif

    static std::atomic<bool> backrefGrowthDone;
    static void *ptrs[BACKREF_GROWTH_ITERS];
public:
    TestInvalidBackrefs() {}
    void operator()(int id) const {

        if (!id) {
            backrefGrowthDone = false;
            barrier.wait();

            for (int i=0; i<BACKREF_GROWTH_ITERS; i++)
                ptrs[i] = scalable_malloc(minLargeObjectSize);
            backrefGrowthDone = true;
            for (int i=0; i<BACKREF_GROWTH_ITERS; i++)
                scalable_free(ptrs[i]);
        } else {
            void *p2 = scalable_malloc(minLargeObjectSize-1);
            char *p1 = (char*)scalable_malloc(minLargeObjectSize-1);
            LargeObjectHdr *hdr =
                (LargeObjectHdr*)(p1+minLargeObjectSize-1 - sizeof(LargeObjectHdr));
            hdr->backRefIdx.main = 7;
            hdr->backRefIdx.largeObj = 1;
            hdr->backRefIdx.offset = 2000;

            barrier.wait();

            int yield_count = 0;
            while (!backrefGrowthDone) {
                scalable_free(p2);
                p2 = scalable_malloc(minLargeObjectSize-1);
                if (yield_count++ == 100) {
                    yield_count = 0;
                    std::this_thread::yield();
                }
            }
            scalable_free(p1);
            scalable_free(p2);
        }
    }
};

std::atomic<bool> TestInvalidBackrefs::backrefGrowthDone;
void *TestInvalidBackrefs::ptrs[BACKREF_GROWTH_ITERS];

void TestBackRef() {
    size_t beforeNumBackRef, afterNumBackRef;

    beforeNumBackRef = allocatedBackRefCount();
    for( int p=MaxThread; p>=MinThread; --p )
        utils::NativeParallelFor( p, BackRefWork<2*BR_MAX_CNT+2>() );
    afterNumBackRef = allocatedBackRefCount();
    REQUIRE_MESSAGE(beforeNumBackRef==afterNumBackRef, "backreference leak detected");
    // lastUsed marks peak resource consumption. As we allocate below the mark,
    // it must not move up, otherwise there is a resource leak.
    int sustLastUsed = backRefMain.load(std::memory_order_relaxed)->lastUsed.load(std::memory_order_relaxed);
    utils::NativeParallelFor( 1, BackRefWork<2*BR_MAX_CNT+2>() );
    REQUIRE_MESSAGE(sustLastUsed == backRefMain.load(std::memory_order_relaxed)->lastUsed.load(std::memory_order_relaxed), "backreference leak detected");
    // check leak of back references while per-thread caches are in use
    // warm up needed to cover bootStrapMalloc call
    utils::NativeParallelFor( 1, LocalCachesHit() );
    beforeNumBackRef = allocatedBackRefCount();
    utils::NativeParallelFor( 2, LocalCachesHit() );
    int res = scalable_allocation_command(TBBMALLOC_CLEAN_ALL_BUFFERS, nullptr);
    REQUIRE(res == TBBMALLOC_OK);
    afterNumBackRef = allocatedBackRefCount();
    REQUIRE_MESSAGE(beforeNumBackRef>=afterNumBackRef, "backreference leak detected");

    // This is a regression test against race condition between backreference
    // extension and checking invalid BackRefIdx.
    // While detecting is object large or small, scalable_free 1st check for
    // large objects, so there is a chance to prepend small object with
    // seems valid BackRefIdx for large objects, and thus trigger the bug.
    TestInvalidBackrefs::initBarrier(MaxThread);
    utils::NativeParallelFor( MaxThread, TestInvalidBackrefs() );
    // Consume all available backrefs and check they work correctly.
    // For now test 32-bit machines only, because for 64-bit memory consumption is too high.
    if (sizeof(uintptr_t) == 4)
        utils::NativeParallelFor( MaxThread, BackRefWork<0>() );
}

void *getMem(intptr_t /*pool_id*/, size_t &bytes)
{
    const size_t BUF_SIZE = 8*1024*1024;
    static char space[BUF_SIZE];
    static size_t pos;

    if (pos + bytes > BUF_SIZE)
        return nullptr;

    void *ret = space + pos;
    pos += bytes;

    return ret;
}

int putMem(intptr_t /*pool_id*/, void* /*raw_ptr*/, size_t /*raw_bytes*/)
{
    return 0;
}

struct MallocPoolHeader {
    void  *rawPtr;
    size_t userSize;
};

void *getMallocMem(intptr_t /*pool_id*/, size_t &bytes)
{
    void *rawPtr = malloc(bytes+sizeof(MallocPoolHeader));
    void *ret = (void *)((uintptr_t)rawPtr+sizeof(MallocPoolHeader));

    MallocPoolHeader *hdr = (MallocPoolHeader*)ret-1;
    hdr->rawPtr = rawPtr;
    hdr->userSize = bytes;

    return ret;
}

int putMallocMem(intptr_t /*pool_id*/, void *ptr, size_t bytes)
{
    MallocPoolHeader *hdr = (MallocPoolHeader*)ptr-1;
    ASSERT(bytes == hdr->userSize, "Invalid size in pool callback.");
    free(hdr->rawPtr);

    return 0;
}

class StressLOCacheWork: utils::NoAssign {
    rml::MemoryPool *my_mallocPool;
public:
    StressLOCacheWork(rml::MemoryPool *mallocPool) : my_mallocPool(mallocPool) {}
    void operator()(int) const {
        for (size_t sz=minLargeObjectSize; sz<1*1024*1024;
             sz+=LargeObjectCache::LargeBSProps::CacheStep) {
            void *ptr = pool_malloc(my_mallocPool, sz);
            REQUIRE_MESSAGE(ptr, "Memory was not allocated");
            memset(ptr, sz, sz);
            pool_free(my_mallocPool, ptr);
        }
    }
};

void TestPools() {
    rml::MemPoolPolicy pol(getMem, putMem);
    size_t beforeNumBackRef, afterNumBackRef;

    rml::MemoryPool *pool1;
    rml::MemoryPool *pool2;
    pool_create_v1(0, &pol, &pool1);
    pool_create_v1(0, &pol, &pool2);
    pool_destroy(pool1);
    pool_destroy(pool2);

    scalable_allocation_command(TBBMALLOC_CLEAN_ALL_BUFFERS, nullptr);
    beforeNumBackRef = allocatedBackRefCount();
    rml::MemoryPool *fixedPool;

    pool_create_v1(0, &pol, &fixedPool);
    pol.pAlloc = getMallocMem;
    pol.pFree = putMallocMem;
    pol.granularity = 8;
    rml::MemoryPool *mallocPool;

    pool_create_v1(0, &pol, &mallocPool);
/* check that large object cache (LOC) returns correct size for cached objects
   passBackendSz Byte objects are cached in LOC, but bypassed the backend, so
   memory requested directly from allocation callback.
   nextPassBackendSz Byte objects must fit to another LOC bin,
   so that their allocation/realeasing leads to cache cleanup.
   All this is expecting to lead to releasing of passBackendSz Byte object
   from LOC during LOC cleanup, and putMallocMem checks that returned size
   is correct.
*/
    const size_t passBackendSz = Backend::maxBinned_HugePage+1,
        anotherLOCBinSz = minLargeObjectSize+1;
    for (int i=0; i<10; i++) { // run long enough to be cached
        void *p = pool_malloc(mallocPool, passBackendSz);
        REQUIRE_MESSAGE(p, "Memory was not allocated");
        pool_free(mallocPool, p);
    }
    // run long enough to passBackendSz allocation was cleaned from cache
    // and returned back to putMallocMem for size checking
    for (int i=0; i<1000; i++) {
        void *p = pool_malloc(mallocPool, anotherLOCBinSz);
        REQUIRE_MESSAGE(p, "Memory was not allocated");
        pool_free(mallocPool, p);
    }

    void *smallObj =  pool_malloc(fixedPool, 10);
    REQUIRE_MESSAGE(smallObj, "Memory was not allocated");
    memset(smallObj, 1, 10);
    void *ptr = pool_malloc(fixedPool, 1024);
    REQUIRE_MESSAGE(ptr, "Memory was not allocated");
    memset(ptr, 1, 1024);
    void *largeObj = pool_malloc(fixedPool, minLargeObjectSize);
    REQUIRE_MESSAGE(largeObj, "Memory was not allocated");
    memset(largeObj, 1, minLargeObjectSize);
    ptr = pool_malloc(fixedPool, minLargeObjectSize);
    REQUIRE_MESSAGE(ptr, "Memory was not allocated");
    memset(ptr, minLargeObjectSize, minLargeObjectSize);
    pool_malloc(fixedPool, 10*minLargeObjectSize); // no leak for unsuccessful allocations
    pool_free(fixedPool, smallObj);
    pool_free(fixedPool, largeObj);

    // provoke large object cache cleanup and hope no leaks occurs
    for( int p=MaxThread; p>=MinThread; --p )
        utils::NativeParallelFor( p, StressLOCacheWork(mallocPool) );
    pool_destroy(mallocPool);
    pool_destroy(fixedPool);

    scalable_allocation_command(TBBMALLOC_CLEAN_ALL_BUFFERS, nullptr);
    afterNumBackRef = allocatedBackRefCount();
    REQUIRE_MESSAGE(beforeNumBackRef==afterNumBackRef, "backreference leak detected");

    {
        // test usedSize/cachedSize and LOC bitmask correctness
        void *p[5];
        pool_create_v1(0, &pol, &mallocPool);
        const LargeObjectCache *loc = &((rml::internal::MemoryPool*)mallocPool)->extMemPool.loc;
        const int LargeCacheStep = LargeObjectCache::LargeBSProps::CacheStep;
        p[3] = pool_malloc(mallocPool, minLargeObjectSize+2*LargeCacheStep);
        for (int i=0; i<10; i++) {
            p[0] = pool_malloc(mallocPool, minLargeObjectSize);
            p[1] = pool_malloc(mallocPool, minLargeObjectSize+LargeCacheStep);
            pool_free(mallocPool, p[0]);
            pool_free(mallocPool, p[1]);
        }
        REQUIRE(loc->getUsedSize());
        pool_free(mallocPool, p[3]);
        REQUIRE(loc->getLOCSize() < 3*(minLargeObjectSize+LargeCacheStep));
        const size_t maxLocalLOCSize = LocalLOCImpl<3,30>::getMaxSize();
        REQUIRE(loc->getUsedSize() <= maxLocalLOCSize);
        for (int i=0; i<3; i++)
            p[i] = pool_malloc(mallocPool, minLargeObjectSize+i*LargeCacheStep);
        size_t currUser = loc->getUsedSize();
        REQUIRE((!loc->getLOCSize() && currUser >= 3*(minLargeObjectSize+LargeCacheStep)));
        p[4] = pool_malloc(mallocPool, minLargeObjectSize+3*LargeCacheStep);
        REQUIRE(loc->getUsedSize() - currUser >= minLargeObjectSize+3*LargeCacheStep);
        pool_free(mallocPool, p[4]);
        REQUIRE(loc->getUsedSize() <= currUser+maxLocalLOCSize);
        pool_reset(mallocPool);
        REQUIRE((!loc->getLOCSize() && !loc->getUsedSize()));
        pool_destroy(mallocPool);
    }
    // To test LOC we need bigger lists than released by current LocalLOC
    //   in production code. Create special LocalLOC.
    {
        LocalLOCImpl<2, 20> lLOC;
        pool_create_v1(0, &pol, &mallocPool);
        rml::internal::ExtMemoryPool *mPool = &((rml::internal::MemoryPool*)mallocPool)->extMemPool;
        const LargeObjectCache *loc = &((rml::internal::MemoryPool*)mallocPool)->extMemPool.loc;
        const int LargeCacheStep = LargeObjectCache::LargeBSProps::CacheStep;
        for (int i=0; i<22; i++) {
            void *o = pool_malloc(mallocPool, minLargeObjectSize+i*LargeCacheStep);
            bool ret = lLOC.put(((LargeObjectHdr*)o - 1)->memoryBlock, mPool);
            REQUIRE(ret);

            o = pool_malloc(mallocPool, minLargeObjectSize+i*LargeCacheStep);
            ret = lLOC.put(((LargeObjectHdr*)o - 1)->memoryBlock, mPool);
            REQUIRE(ret);
        }
        lLOC.externalCleanup(mPool);
        REQUIRE(!loc->getUsedSize());

        pool_destroy(mallocPool);
    }
}

void TestObjectRecognition() {
    size_t headersSize = sizeof(LargeMemoryBlock)+sizeof(LargeObjectHdr);
    unsigned falseObjectSize = 113; // unsigned is the type expected by getObjectSize
    size_t obtainedSize;

    REQUIRE_MESSAGE(sizeof(BackRefIdx)==sizeof(uintptr_t), "Unexpected size of BackRefIdx");
    REQUIRE_MESSAGE(getObjectSize(falseObjectSize)!=falseObjectSize, "Error in test: bad choice for false object size");

    void* mem = scalable_malloc(2*slabSize);
    REQUIRE_MESSAGE(mem, "Memory was not allocated");
    Block* falseBlock = (Block*)alignUp((uintptr_t)mem, slabSize);
    falseBlock->objectSize = falseObjectSize;
    char* falseSO = (char*)falseBlock + falseObjectSize*7;
    REQUIRE_MESSAGE(alignDown(falseSO, slabSize)==(void*)falseBlock, "Error in test: false object offset is too big");

    void* bufferLOH = scalable_malloc(2*slabSize + headersSize);
    REQUIRE_MESSAGE(bufferLOH, "Memory was not allocated");
    LargeObjectHdr* falseLO =
        (LargeObjectHdr*)alignUp((uintptr_t)bufferLOH + headersSize, slabSize);
    LargeObjectHdr* headerLO = (LargeObjectHdr*)falseLO-1;
    headerLO->memoryBlock = (LargeMemoryBlock*)bufferLOH;
    headerLO->memoryBlock->unalignedSize = 2*slabSize + headersSize;
    headerLO->memoryBlock->objectSize = slabSize + headersSize;
    headerLO->backRefIdx = BackRefIdx::newBackRef(/*largeObj=*/true);
    setBackRef(headerLO->backRefIdx, headerLO);
    REQUIRE_MESSAGE(scalable_msize(falseLO) == slabSize + headersSize,
           "Error in test: LOH falsification failed");
    removeBackRef(headerLO->backRefIdx);

    const int NUM_OF_IDX = BR_MAX_CNT+2;
    BackRefIdx idxs[NUM_OF_IDX];
    for (int cnt=0; cnt<2; cnt++) {
        for (int main = -10; main<10; main++) {
            falseBlock->backRefIdx.main = (uint16_t)main;
            headerLO->backRefIdx.main = (uint16_t)main;

            for (int bl = -10; bl<BR_MAX_CNT+10; bl++) {
                falseBlock->backRefIdx.offset = (uint16_t)bl;
                headerLO->backRefIdx.offset = (uint16_t)bl;

                for (int largeObj = 0; largeObj<2; largeObj++) {
                    falseBlock->backRefIdx.largeObj = largeObj;
                    headerLO->backRefIdx.largeObj = largeObj;

                    obtainedSize = __TBB_malloc_safer_msize(falseSO, nullptr);
                    REQUIRE_MESSAGE(obtainedSize==0, "Incorrect pointer accepted");
                    obtainedSize = __TBB_malloc_safer_msize(falseLO, nullptr);
                    REQUIRE_MESSAGE(obtainedSize==0, "Incorrect pointer accepted");
                }
            }
        }
        if (cnt == 1) {
            for (int i=0; i<NUM_OF_IDX; i++)
                removeBackRef(idxs[i]);
            break;
        }
        for (int i=0; i<NUM_OF_IDX; i++) {
            idxs[i] = BackRefIdx::newBackRef(/*largeObj=*/false);
            setBackRef(idxs[i], nullptr);
        }
    }
    char *smallPtr = (char*)scalable_malloc(falseObjectSize);
    obtainedSize = __TBB_malloc_safer_msize(smallPtr, nullptr);
    REQUIRE_MESSAGE(obtainedSize==getObjectSize(falseObjectSize), "Correct pointer not accepted?");
    scalable_free(smallPtr);

    obtainedSize = __TBB_malloc_safer_msize(mem, nullptr);
    REQUIRE_MESSAGE(obtainedSize>=2*slabSize, "Correct pointer not accepted?");
    scalable_free(mem);
    scalable_free(bufferLOH);
}

class TestBackendWork: public SimpleBarrier {
    struct TestBlock {
        intptr_t   data;
        BackRefIdx idx;
    };
    static const int ITERS = 20;

    rml::internal::Backend *backend;
public:
    TestBackendWork(rml::internal::Backend *bknd) : backend(bknd) {}
    void operator()(int) const {
        barrier.wait();

        for (int i=0; i<ITERS; i++) {
            BlockI *slabBlock = backend->getSlabBlock(1);
            REQUIRE_MESSAGE(slabBlock, "Memory was not allocated");
            uintptr_t prevBlock = (uintptr_t)slabBlock;
            backend->putSlabBlock(slabBlock);

            LargeMemoryBlock *largeBlock = backend->getLargeBlock(16*1024);
            REQUIRE_MESSAGE(largeBlock, "Memory was not allocated");
            REQUIRE_MESSAGE((uintptr_t)largeBlock != prevBlock,
                    "Large block cannot be reused from slab memory, only in fixed_pool case.");
            backend->putLargeBlock(largeBlock);
        }
    }
};

void TestBackend()
{
    rml::MemPoolPolicy pol(getMallocMem, putMallocMem);
    rml::MemoryPool *mPool;
    pool_create_v1(0, &pol, &mPool);
    rml::internal::ExtMemoryPool *ePool = &((rml::internal::MemoryPool*)mPool)->extMemPool;
    rml::internal::Backend *backend = &ePool->backend;

    for( int p=MaxThread; p>=MinThread; --p ) {
        // regression test against an race condition in backend synchronization,
        // triggered only when WhiteboxTestingYield() call yields
#if TBB_USE_DEBUG
        int num_iters = 10;
#else
        int num_iters = 100;
#endif
        for (int i = 0; i < num_iters; i++) {
            TestBackendWork::initBarrier(p);
            utils::NativeParallelFor( p, TestBackendWork(backend) );
        }
    }

    BlockI *block = backend->getSlabBlock(1);
    REQUIRE_MESSAGE(block, "Memory was not allocated");
    backend->putSlabBlock(block);

    // Checks if the backend increases and decreases the amount of allocated memory when memory is allocated.
    const size_t memSize0 = backend->getTotalMemSize();
    LargeMemoryBlock *lmb = backend->getLargeBlock(4*MByte);
    REQUIRE( lmb );

    const size_t memSize1 = backend->getTotalMemSize();
    REQUIRE_MESSAGE( (intptr_t)(memSize1-memSize0) >= 4*MByte, "The backend has not increased the amount of using memory." );

    backend->putLargeBlock(lmb);
    const size_t memSize2 = backend->getTotalMemSize();
    REQUIRE_MESSAGE( memSize2 == memSize0, "The backend has not decreased the amount of using memory." );

    pool_destroy(mPool);
}

void TestBitMask()
{
    BitMaskMin<256> mask;

    mask.reset();
    mask.set(10, 1);
    mask.set(5, 1);
    mask.set(1, 1);
    REQUIRE(mask.getMinTrue(2) == 5);

    mask.reset();
    mask.set(0, 1);
    mask.set(64, 1);
    mask.set(63, 1);
    mask.set(200, 1);
    mask.set(255, 1);
    REQUIRE(mask.getMinTrue(0) == 0);
    REQUIRE(mask.getMinTrue(1) == 63);
    REQUIRE(mask.getMinTrue(63) == 63);
    REQUIRE(mask.getMinTrue(64) == 64);
    REQUIRE(mask.getMinTrue(101) == 200);
    REQUIRE(mask.getMinTrue(201) == 255);
    mask.set(255, 0);
    REQUIRE(mask.getMinTrue(201) == -1);
}

size_t getMemSize()
{
    return defaultMemPool->extMemPool.backend.getTotalMemSize();
}

class CheckNotCached {
    static size_t memSize;
public:
    void operator() () const {
        int res = scalable_allocation_mode(TBBMALLOC_SET_SOFT_HEAP_LIMIT, 1);
        REQUIRE(res == TBBMALLOC_OK);
        if (memSize==(size_t)-1) {
            memSize = getMemSize();
        } else {
            REQUIRE(getMemSize() == memSize);
            memSize=(size_t)-1;
        }
    }
};

size_t CheckNotCached::memSize = (size_t)-1;

class RunTestHeapLimit: public SimpleBarrier {
public:
    void operator()( int /*mynum*/ ) const {
        // Provoke bootstrap heap initialization before recording memory size.
        // NOTE: The initialization should be processed only with a "large"
        // object. Since the "small" object allocation lead to blocking of a
        // slab as an active block and it is impossible to release it with
        // foreign thread.
        scalable_free(scalable_malloc(minLargeObjectSize));
        barrier.wait(CheckNotCached());
        for (size_t n = minLargeObjectSize; n < 5*1024*1024; n += 128*1024)
            scalable_free(scalable_malloc(n));
        barrier.wait(CheckNotCached());
    }
};

void TestHeapLimit()
{
    if(!isMallocInitialized()) doInitialization();
    // tiny limit to stop caching
    int res = scalable_allocation_mode(TBBMALLOC_SET_SOFT_HEAP_LIMIT, 1);
    REQUIRE(res == TBBMALLOC_OK);
     // Provoke bootstrap heap initialization before recording memory size.
    scalable_free(scalable_malloc(8));
    size_t n, sizeBefore = getMemSize();

    // Try to provoke call to OS for memory to check that
    // requests are not fulfilled from caches.
    // Single call is not enough here because of backend fragmentation.
    for (n = minLargeObjectSize; n < 10*1024*1024; n += 16*1024) {
        void *p = scalable_malloc(n);
        bool leave = (sizeBefore != getMemSize());
        scalable_free(p);
        if (leave)
            break;
        REQUIRE_MESSAGE(sizeBefore == getMemSize(), "No caching expected");
    }
    REQUIRE_MESSAGE(n < 10*1024*1024, "scalable_malloc doesn't provoke OS request for memory, "
           "is some internal cache still used?");

    for( int p=MaxThread; p>=MinThread; --p ) {
        RunTestHeapLimit::initBarrier( p );
        utils::NativeParallelFor( p, RunTestHeapLimit() );
    }
    // it's try to match limit as well as set limit, so call here
    res = scalable_allocation_mode(TBBMALLOC_SET_SOFT_HEAP_LIMIT, 1);
    REQUIRE(res == TBBMALLOC_OK);
    size_t m = getMemSize();
    REQUIRE(sizeBefore == m);
    // restore default
    res = scalable_allocation_mode(TBBMALLOC_SET_SOFT_HEAP_LIMIT, 0);
    REQUIRE(res == TBBMALLOC_OK);
}

void checkNoHugePages()
{
    REQUIRE_MESSAGE(!hugePages.isEnabled, "scalable_allocation_mode "
           "must have priority over environment variable");
}

/*---------------------------------------------------------------------------*/
// The regression test against bugs in TBBMALLOC_CLEAN_ALL_BUFFERS allocation command.
// The idea is to allocate and deallocate a set of objects randomly in parallel.
// For large sizes (16K), it forces conflicts in backend during coalescing.
// For small sizes (4K), it forces cross-thread deallocations and then orphaned slabs.
// Global cleanup should process orphaned slabs and the queue of postponed coalescing
// requests, otherwise it will not be able to unmap all unused memory.

const int num_allocs = 10*1024;
void *ptrs[num_allocs];
std::atomic<int> alloc_counter;
static thread_local bool free_was_called = false;

inline void multiThreadAlloc(size_t alloc_size) {
    for( int i = alloc_counter++; i < num_allocs; i = alloc_counter++ ) {
       ptrs[i] = scalable_malloc( alloc_size );
       REQUIRE_MESSAGE( ptrs[i] != nullptr, "scalable_malloc returned zero." );
    }
}
inline void crossThreadDealloc() {
    free_was_called = false;
    for( int i = --alloc_counter; i >= 0; i = --alloc_counter ) {
        if (i < num_allocs) {
            scalable_free(ptrs[i]);
            free_was_called = true;
        }
    }
}

template<int AllocSize>
struct TestCleanAllBuffersBody : public SimpleBarrier {
    void operator() ( int ) const {
        barrier.wait();
        multiThreadAlloc(AllocSize);
        barrier.wait();
        crossThreadDealloc();
    }
};

template<int AllocSize>
void TestCleanAllBuffers() {
    const int num_threads = 8;
    // Clean up if something was allocated before the test
    scalable_allocation_command(TBBMALLOC_CLEAN_ALL_BUFFERS,nullptr);

    size_t memory_in_use_before = getMemSize();
    alloc_counter = 0;
    TestCleanAllBuffersBody<AllocSize>::initBarrier(num_threads);

    utils::NativeParallelFor(num_threads, TestCleanAllBuffersBody<AllocSize>());
    // TODO: reproduce the bug conditions more reliably
    if ( defaultMemPool->extMemPool.backend.coalescQ.blocksToFree.load(std::memory_order_relaxed) == nullptr ) {
        INFO( "Warning: The queue of postponed coalescing requests is empty. ");
        INFO( "Unable to create the condition for bug reproduction.\n" );
    }
    int result = scalable_allocation_command(TBBMALLOC_CLEAN_ALL_BUFFERS,nullptr);
    REQUIRE_MESSAGE( result == TBBMALLOC_OK, "The cleanup request has not cleaned anything." );
    size_t memory_in_use_after = getMemSize();

    size_t memory_leak = memory_in_use_after - memory_in_use_before;
    INFO( "memory_in_use_before = " <<  memory_in_use_before << ", memory_in_use_after = " << memory_in_use_after << "\n" );
    REQUIRE_MESSAGE( memory_leak == 0, "Cleanup was unable to release all allocated memory." );
}

//! Force cross thread deallocation of small objects to create a set of privatizable slab blocks.
//! TBBMALLOC_CLEAN_THREAD_BUFFERS command have to privatize all the block.
struct TestCleanThreadBuffersBody : public SimpleBarrier {
    void operator() ( int ) const {
        barrier.wait();
        multiThreadAlloc(2*1024);
        barrier.wait();
        crossThreadDealloc();
        barrier.wait();
        int result = scalable_allocation_command(TBBMALLOC_CLEAN_THREAD_BUFFERS,nullptr);
        if (result != TBBMALLOC_OK && free_was_called) {
            REPORT("Warning: clean-up request for this particular thread has not cleaned anything.");
        }

        // Check that TLS was cleaned fully
        TLSData *tlsCurr = defaultMemPool->getTLS(/*create=*/false);
        if (tlsCurr) {
            for (int i = 0; i < numBlockBinLimit; i++) {
                REQUIRE_MESSAGE(!(tlsCurr->bin[i].activeBlk), "Some bin was not cleaned.");
            }
            REQUIRE_MESSAGE(!(tlsCurr->lloc.head.load(std::memory_order_relaxed)), "Local LOC was not cleaned.");
            REQUIRE_MESSAGE(!(tlsCurr->freeSlabBlocks.head.load(std::memory_order_relaxed)), "Free Block pool was not cleaned.");
        }
    }
};

void TestCleanThreadBuffers() {
    const int num_threads = 8;
    // Clean up if something was allocated before the test
    scalable_allocation_command(TBBMALLOC_CLEAN_ALL_BUFFERS,nullptr);

    alloc_counter = 0;
    TestCleanThreadBuffersBody::initBarrier(num_threads);
    utils::NativeParallelFor(num_threads, TestCleanThreadBuffersBody());
}

/*---------------------------------------------------------------------------*/
/*------------------------- Large Object Cache tests ------------------------*/
#if _MSC_VER==1600 || _MSC_VER==1500
    // ignore C4275: non dll-interface class 'stdext::exception' used as
    // base for dll-interface class 'std::bad_cast'
    #pragma warning (disable: 4275)
#endif
#include <vector>
#include <list>

// default constructor of CacheBin
template<typename Props>
rml::internal::LargeObjectCacheImpl<Props>::CacheBin::CacheBin() {}

template<typename Props>
class CacheBinModel {

    typedef typename rml::internal::LargeObjectCacheImpl<Props>::CacheBin CacheBinType;

    // The emulated cache bin.
    CacheBinType cacheBinModel;
    // The reference to real cache bin inside the large object cache.
    CacheBinType &cacheBin;

    const size_t size;

    // save only current time
    std::list<uintptr_t> objects;

    void doCleanup() {
        if ( cacheBinModel.cachedSize.load(std::memory_order_relaxed) >
            Props::TooLargeFactor*cacheBinModel.usedSize.load(std::memory_order_relaxed)) tooLargeLOC++;
        else tooLargeLOC = 0;

        intptr_t threshold = cacheBinModel.ageThreshold.load(std::memory_order_relaxed);
        if (tooLargeLOC > 3 && threshold) {
            threshold = (threshold + cacheBinModel.meanHitRange.load(std::memory_order_relaxed)) / 2;
            cacheBinModel.ageThreshold.store(threshold, std::memory_order_relaxed);
        }

        uintptr_t currTime = cacheCurrTime;
        while (!objects.empty() && (intptr_t)(currTime - objects.front()) > threshold) {
            cacheBinModel.cachedSize.store(cacheBinModel.cachedSize.load(std::memory_order_relaxed) - size, std::memory_order_relaxed);
            cacheBinModel.lastCleanedAge = objects.front();
            objects.pop_front();
        }

        cacheBinModel.oldest.store(objects.empty() ? 0 : objects.front(), std::memory_order_relaxed);
    }

public:
    CacheBinModel(CacheBinType &_cacheBin, size_t allocSize) : cacheBin(_cacheBin), size(allocSize) {
        cacheBinModel.oldest.store(cacheBin.oldest.load(std::memory_order_relaxed), std::memory_order_relaxed);
        cacheBinModel.lastCleanedAge = cacheBin.lastCleanedAge;
        cacheBinModel.ageThreshold.store(cacheBin.ageThreshold.load(std::memory_order_relaxed), std::memory_order_relaxed);
        cacheBinModel.usedSize.store(cacheBin.usedSize.load(std::memory_order_relaxed), std::memory_order_relaxed);
        cacheBinModel.cachedSize.store(cacheBin.cachedSize.load(std::memory_order_relaxed), std::memory_order_relaxed);
        cacheBinModel.meanHitRange.store(cacheBin.meanHitRange.load(std::memory_order_relaxed), std::memory_order_relaxed);
        cacheBinModel.lastGet = cacheBin.lastGet;
    }
    void get() {
        uintptr_t currTime = ++cacheCurrTime;

        if ( objects.empty() ) {
            const uintptr_t sinceLastGet = currTime - cacheBinModel.lastGet;
            intptr_t threshold = cacheBinModel.ageThreshold.load(std::memory_order_relaxed);
            if ((threshold && sinceLastGet > Props::LongWaitFactor * threshold) ||
                (cacheBinModel.lastCleanedAge && sinceLastGet > Props::LongWaitFactor * (cacheBinModel.lastCleanedAge - cacheBinModel.lastGet))) {
                cacheBinModel.lastCleanedAge = 0;
                cacheBinModel.ageThreshold.store(0, std::memory_order_relaxed);
            }

            if (cacheBinModel.lastCleanedAge)
                cacheBinModel.ageThreshold.store(Props::OnMissFactor * (currTime - cacheBinModel.lastCleanedAge), std::memory_order_relaxed);
        } else {
            uintptr_t obj_age = objects.back();
            objects.pop_back();
            if (objects.empty()) cacheBinModel.oldest.store(0, std::memory_order_relaxed);

            intptr_t hitRange = currTime - obj_age;
            intptr_t mean = cacheBinModel.meanHitRange.load(std::memory_order_relaxed);
            mean = mean ? (mean + hitRange) / 2 : hitRange;
            cacheBinModel.meanHitRange.store(mean, std::memory_order_relaxed);

            cacheBinModel.cachedSize.store(cacheBinModel.cachedSize.load(std::memory_order_relaxed) - size, std::memory_order_relaxed);
        }

        cacheBinModel.usedSize.store(cacheBinModel.usedSize.load(std::memory_order_relaxed) + size, std::memory_order_relaxed);
        cacheBinModel.lastGet = currTime;

        if ( currTime % rml::internal::cacheCleanupFreq == 0 ) doCleanup();
    }

    void putList( int num ) {
        uintptr_t currTime = cacheCurrTime;
        cacheCurrTime += num;

        cacheBinModel.usedSize.store(cacheBinModel.usedSize.load(std::memory_order_relaxed) - num * size, std::memory_order_relaxed);

        bool cleanUpNeeded = false;
        if ( !cacheBinModel.lastCleanedAge ) {
            cacheBinModel.lastCleanedAge = ++currTime;
            cleanUpNeeded |= currTime % rml::internal::cacheCleanupFreq == 0;
            num--;
        }

        for ( int i=1; i<=num; ++i ) {
            currTime+=1;
            cleanUpNeeded |= currTime % rml::internal::cacheCleanupFreq == 0;
            if (objects.empty())
                cacheBinModel.oldest.store(currTime, std::memory_order_relaxed);
            objects.push_back(currTime);
        }

        cacheBinModel.cachedSize.store(cacheBinModel.cachedSize.load(std::memory_order_relaxed) + num * size, std::memory_order_relaxed);

        if ( cleanUpNeeded ) doCleanup();
    }

    void check() {
        CHECK_FAST(cacheBinModel.oldest.load(std::memory_order_relaxed) == cacheBin.oldest.load(std::memory_order_relaxed));
        CHECK_FAST(cacheBinModel.lastCleanedAge == cacheBin.lastCleanedAge);
        CHECK_FAST(cacheBinModel.ageThreshold.load(std::memory_order_relaxed) == cacheBin.ageThreshold.load(std::memory_order_relaxed));
        CHECK_FAST(cacheBinModel.usedSize.load(std::memory_order_relaxed) == cacheBin.usedSize.load(std::memory_order_relaxed));
        CHECK_FAST(cacheBinModel.cachedSize.load(std::memory_order_relaxed) == cacheBin.cachedSize.load(std::memory_order_relaxed));
        CHECK_FAST(cacheBinModel.meanHitRange.load(std::memory_order_relaxed) == cacheBin.meanHitRange.load(std::memory_order_relaxed));
        CHECK_FAST(cacheBinModel.lastGet == cacheBin.lastGet);
    }

    static uintptr_t cacheCurrTime;
    static intptr_t tooLargeLOC;
};

template<typename Props> uintptr_t CacheBinModel<Props>::cacheCurrTime;
template<typename Props> intptr_t CacheBinModel<Props>::tooLargeLOC;

template <typename Scenario>
void LOCModelTester() {
    defaultMemPool->extMemPool.loc.cleanAll();
    defaultMemPool->extMemPool.loc.reset();

    const size_t size = 16 * 1024;
    const size_t headersSize = sizeof(rml::internal::LargeMemoryBlock)+sizeof(rml::internal::LargeObjectHdr);
    const size_t allocationSize = LargeObjectCache::alignToBin(size+headersSize+rml::internal::largeObjectAlignment);
    const int binIdx = defaultMemPool->extMemPool.loc.largeCache.sizeToIdx( allocationSize );

    CacheBinModel<rml::internal::LargeObjectCache::LargeCacheTypeProps>::cacheCurrTime = defaultMemPool->extMemPool.loc.cacheCurrTime;
    CacheBinModel<rml::internal::LargeObjectCache::LargeCacheTypeProps>::tooLargeLOC = defaultMemPool->extMemPool.loc.largeCache.tooLargeLOC;
    CacheBinModel<rml::internal::LargeObjectCache::LargeCacheTypeProps> cacheBinModel(defaultMemPool->extMemPool.loc.largeCache.bin[binIdx], allocationSize);

    Scenario scen;
    for (rml::internal::LargeMemoryBlock *lmb = scen.next(); (intptr_t)lmb != (intptr_t)-1; lmb = scen.next()) {
        if ( lmb ) {
            int num=1;
            for (rml::internal::LargeMemoryBlock *curr = lmb; curr->next; curr=curr->next) num+=1;
            defaultMemPool->extMemPool.freeLargeObject(lmb);
            cacheBinModel.putList(num);
        } else {
            scen.saveLmb(defaultMemPool->extMemPool.mallocLargeObject(defaultMemPool, allocationSize));
            cacheBinModel.get();
        }

        cacheBinModel.check();
    }
}

class TestBootstrap {
    bool allocating;
    std::vector<rml::internal::LargeMemoryBlock*> lmbArray;
public:
    TestBootstrap() : allocating(true) {}

    rml::internal::LargeMemoryBlock* next() {
        if ( allocating )
            return nullptr;
        if ( !lmbArray.empty() ) {
            rml::internal::LargeMemoryBlock *ret = lmbArray.back();
            lmbArray.pop_back();
            return ret;
        }
        return (rml::internal::LargeMemoryBlock*)-1;
    }

    void saveLmb( rml::internal::LargeMemoryBlock *lmb ) {
        lmb->next = nullptr;
        lmbArray.push_back(lmb);
        if ( lmbArray.size() == 1000 ) allocating = false;
    }
};

class TestRandom {
    std::vector<rml::internal::LargeMemoryBlock*> lmbArray;
    int numOps;
public:
    TestRandom() : numOps(100000) {
        srand(1234);
    }

    rml::internal::LargeMemoryBlock* next() {
        if ( numOps-- ) {
            if ( lmbArray.empty() || rand() / (RAND_MAX>>1) == 0 )
                return nullptr;
            size_t ind = rand()%lmbArray.size();
            if ( ind != lmbArray.size()-1 ) std::swap(lmbArray[ind],lmbArray[lmbArray.size()-1]);
            rml::internal::LargeMemoryBlock *lmb = lmbArray.back();
            lmbArray.pop_back();
            return lmb;
        }
        return (rml::internal::LargeMemoryBlock*)-1;
    }

    void saveLmb( rml::internal::LargeMemoryBlock *lmb ) {
        lmb->next = nullptr;
        lmbArray.push_back(lmb);
    }
};

class TestCollapsingMallocFree : public SimpleBarrier {
public:
    static const int NUM_ALLOCS = 100000;
    const int num_threads;

    TestCollapsingMallocFree( int _num_threads ) : num_threads(_num_threads) {
        initBarrier( num_threads );
    }

    void operator() ( int ) const {
        const size_t size = 16 * 1024;
        const size_t headersSize = sizeof(rml::internal::LargeMemoryBlock)+sizeof(rml::internal::LargeObjectHdr);
        const size_t allocationSize = LargeObjectCache::alignToBin(size+headersSize+rml::internal::largeObjectAlignment);

        barrier.wait();
        for ( int i=0; i<NUM_ALLOCS; ++i ) {
            defaultMemPool->extMemPool.freeLargeObject(
                defaultMemPool->extMemPool.mallocLargeObject(defaultMemPool, allocationSize) );
        }
    }

    void check() {
        REQUIRE( tbbmalloc_whitebox::locGetProcessed == tbbmalloc_whitebox::locPutProcessed);
        REQUIRE_MESSAGE( tbbmalloc_whitebox::locGetProcessed < num_threads*NUM_ALLOCS, "No one Malloc/Free pair was collapsed." );
    }
};

class TestCollapsingBootstrap : public SimpleBarrier {
    class CheckNumAllocs {
        const int num_threads;
    public:
        CheckNumAllocs( int _num_threads ) : num_threads(_num_threads) {}
        void operator()() const {
            REQUIRE( tbbmalloc_whitebox::locGetProcessed == num_threads*NUM_ALLOCS );
            REQUIRE( tbbmalloc_whitebox::locPutProcessed == 0 );
        }
    };
public:
    static const int NUM_ALLOCS = 1000;
    const int num_threads;

    TestCollapsingBootstrap( int _num_threads ) : num_threads(_num_threads) {
        initBarrier( num_threads );
    }

    void operator() ( int ) const {
        const size_t size = 16 * 1024;
        size_t headersSize = sizeof(rml::internal::LargeMemoryBlock)+sizeof(rml::internal::LargeObjectHdr);
        size_t allocationSize = LargeObjectCache::alignToBin(size+headersSize+rml::internal::largeObjectAlignment);

        barrier.wait();
        rml::internal::LargeMemoryBlock *lmbArray[NUM_ALLOCS];
        for ( int i=0; i<NUM_ALLOCS; ++i )
            lmbArray[i] = defaultMemPool->extMemPool.mallocLargeObject(defaultMemPool, allocationSize);

        barrier.wait(CheckNumAllocs(num_threads));
        for ( int i=0; i<NUM_ALLOCS; ++i )
            defaultMemPool->extMemPool.freeLargeObject( lmbArray[i] );
    }

    void check() {
        REQUIRE( tbbmalloc_whitebox::locGetProcessed == tbbmalloc_whitebox::locPutProcessed );
        REQUIRE( tbbmalloc_whitebox::locGetProcessed == num_threads*NUM_ALLOCS );
    }
};

template <typename Scenario>
void LOCCollapsingTester( int num_threads ) {
    tbbmalloc_whitebox::locGetProcessed = 0;
    tbbmalloc_whitebox::locPutProcessed = 0;
    defaultMemPool->extMemPool.loc.cleanAll();
    defaultMemPool->extMemPool.loc.reset();

    Scenario scen(num_threads);
    utils::NativeParallelFor(num_threads, scen);

    scen.check();
}

void TestLOC() {
    LOCModelTester<TestBootstrap>();
    LOCModelTester<TestRandom>();

    const int num_threads = 16;
    LOCCollapsingTester<TestCollapsingBootstrap>( num_threads );
    if ( num_threads > 1 ) {
        INFO( "num_threads = " << num_threads );
        LOCCollapsingTester<TestCollapsingMallocFree>( num_threads );
    } else {
        REPORT( "Warning: concurrency is too low for TestMallocFreeCollapsing ( num_threads = %d )\n", num_threads );
    }
}
/*---------------------------------------------------------------------------*/

void *findCacheLine(void *p) {
    return (void*)alignDown((uintptr_t)p, estimatedCacheLineSize);
}

// test that internals of Block are at expected cache lines
void TestSlabAlignment() {
    const size_t min_sz = 8;
    const int space = 2*16*1024; // fill at least 2 slabs
    void *pointers[space / min_sz];  // the worst case is min_sz byte object

    for (size_t sz = min_sz; sz <= 64; sz *= 2) {
        for (size_t i = 0; i < space/sz; i++) {
            pointers[i] = scalable_malloc(sz);
            Block *block = (Block *)alignDown(pointers[i], slabSize);
            REQUIRE_MESSAGE(findCacheLine(&block->isFull) != findCacheLine(pointers[i]),
                          "A user object must not share a cache line with slab control structures.");
            REQUIRE_MESSAGE(findCacheLine(&block->next) != findCacheLine(&block->nextPrivatizable),
                          "GlobalBlockFields and LocalBlockFields must be on different cache lines.");
        }
        for (size_t i = 0; i < space/sz; i++)
            scalable_free(pointers[i]);
    }
}

#include "common/memory_usage.h"

// TODO: Consider adding Huge Pages support on macOS (special mmap flag).
// Transparent Huge pages support could be enabled by different system parsing mechanism,
// because there is no /proc/meminfo on macOS
#if __unix__
void TestTHP() {
    // Get backend from default memory pool
    rml::internal::Backend *backend = &(defaultMemPool->extMemPool.backend);

    // Configure malloc to use huge pages
    scalable_allocation_mode(USE_HUGE_PAGES, 1);
    REQUIRE_MESSAGE(hugePages.isEnabled, "Huge pages should be enabled via scalable_allocation_mode");

    const int HUGE_PAGE_SIZE = 2 * 1024 * 1024;

    // allocCount transparent huge pages should be allocated
    const int allocCount = 10;

    // Allocate huge page aligned memory regions to track system
    // counters for transparent huge pages
    void*  allocPtrs[allocCount];

    // Wait for the system to update process memory info files after other tests
    utils::Sleep(4000);

    // Parse system info regarding current THP status
    size_t currentSystemTHPCount = utils::getSystemTHPCount();
    size_t currentSystemTHPAllocatedSize = utils::getSystemTHPAllocatedSize();

    for (int i = 0; i < allocCount; i++) {
        // Allocation size have to be aligned on page size
        size_t allocSize = HUGE_PAGE_SIZE - (i * 1000);

        // Map memory
        allocPtrs[i] = backend->allocRawMem(allocSize);

        REQUIRE_MESSAGE(allocPtrs[i], "Allocation not succeeded.");
        REQUIRE_MESSAGE(allocSize == HUGE_PAGE_SIZE,
            "Allocation size have to be aligned on Huge Page size internally.");

        // First touch policy - no real pages allocated by OS without accessing the region
        memset(allocPtrs[i], 1, allocSize);

        REQUIRE_MESSAGE(isAligned(allocPtrs[i], HUGE_PAGE_SIZE),
            "The pointer returned by scalable_malloc is not aligned on huge page size.");
    }

    // Wait for the system to update process memory info files after allocations
    utils::Sleep(4000);

    // Generally, kernel tries to allocate transparent huge pages, but sometimes it cannot do this
    // (tested on SLES 11/12), so consider this system info checks as a remark.
    // Also, some systems can allocate more memory then needed in background (tested on Ubuntu 14.04)
    size_t newSystemTHPCount = utils::getSystemTHPCount();
    size_t newSystemTHPAllocatedSize = utils::getSystemTHPAllocatedSize();
    if ((newSystemTHPCount - currentSystemTHPCount) < allocCount
            && (newSystemTHPAllocatedSize - currentSystemTHPAllocatedSize) / (2 * 1024) < allocCount) {
        REPORT( "Warning: the system didn't allocate needed amount of THPs.\n" );
    }

    // Test memory unmap
    for (int i = 0; i < allocCount; i++) {
        REQUIRE_MESSAGE(backend->freeRawMem(allocPtrs[i], HUGE_PAGE_SIZE),
                "Something went wrong during raw memory free");
    }
}
#endif // __unix__

inline size_t getStabilizedMemUsage() {
    for (int i = 0; i < 3; i++) utils::GetMemoryUsage();
    return utils::GetMemoryUsage();
}

inline void* reallocAndRetrieve(void* origPtr, size_t reallocSize, size_t& origBlockSize, size_t& reallocBlockSize) {
    rml::internal::LargeMemoryBlock* origLmb = ((rml::internal::LargeObjectHdr *)origPtr - 1)->memoryBlock;
    origBlockSize = origLmb->unalignedSize;

    void* reallocPtr = rml::internal::reallocAligned(defaultMemPool, origPtr, reallocSize, 0);

    // Retrieved reallocated block information
    rml::internal::LargeMemoryBlock* reallocLmb = ((rml::internal::LargeObjectHdr *)reallocPtr - 1)->memoryBlock;
    reallocBlockSize = reallocLmb->unalignedSize;

    return reallocPtr;
}

void TestReallocDecreasing() {

    /* Testing that actual reallocation happens for large objects that do not fit the backend cache
       but decrease in size by a factor of >= 2. */

    size_t startSize = 100 * 1024 * 1024;
    size_t maxBinnedSize = defaultMemPool->extMemPool.backend.getMaxBinnedSize();
    void*  origPtr = scalable_malloc(startSize);
    void*  reallocPtr = nullptr;

    // Realloc on 1MB less size
    size_t origBlockSize = 42;
    size_t reallocBlockSize = 43;
    reallocPtr = reallocAndRetrieve(origPtr, startSize - 1 * 1024 * 1024, origBlockSize, reallocBlockSize);
    REQUIRE_MESSAGE(origBlockSize == reallocBlockSize, "Reallocated block size shouldn't change");
    REQUIRE_MESSAGE(reallocPtr == origPtr, "Original pointer shouldn't change");

    // Repeated decreasing reallocation while max cache bin size reached
    size_t reallocSize = (startSize / 2) - 1000; // exact realloc
    while(reallocSize > maxBinnedSize) {

        // Prevent huge/large objects caching
        defaultMemPool->extMemPool.loc.cleanAll();
        // Prevent local large object caching
        TLSData *tls = defaultMemPool->getTLS(/*create=*/false);
        tls->lloc.externalCleanup(&defaultMemPool->extMemPool);

        size_t sysMemUsageBefore = getStabilizedMemUsage();
        size_t totalMemSizeBefore = defaultMemPool->extMemPool.backend.getTotalMemSize();

        reallocPtr = reallocAndRetrieve(origPtr, reallocSize, origBlockSize, reallocBlockSize);

        REQUIRE_MESSAGE(origBlockSize > reallocBlockSize, "Reallocated block size should descrease.");

        size_t sysMemUsageAfter = getStabilizedMemUsage();
        size_t totalMemSizeAfter = defaultMemPool->extMemPool.backend.getTotalMemSize();

        // Prevent false checking when backend caching occurred or could not read system memory usage info
        if (totalMemSizeBefore > totalMemSizeAfter && sysMemUsageAfter != 0 && sysMemUsageBefore != 0) {
            REQUIRE_MESSAGE(sysMemUsageBefore > sysMemUsageAfter, "Memory were not released");
        }

        origPtr = reallocPtr;
        reallocSize = (reallocSize / 2) - 1000; // exact realloc
    }
    scalable_free(reallocPtr);

    /* TODO: Decreasing reallocation of large objects that fit backend cache */
    /* TODO: Small objects decreasing reallocation test */
}
#if !__TBB_WIN8UI_SUPPORT && defined(_WIN32)

#include "../../src/tbbmalloc_proxy/function_replacement.cpp"
#include <string>
namespace FunctionReplacement {
    FunctionInfo funcInfo = { "funcname","dllname" };
    char **func_replacement_log;
    int status;

    void LogCleanup() {
        // Free all allocated memory
        for (unsigned i = 0; i < Log::record_number; i++){
            HeapFree(GetProcessHeap(), 0, Log::records[i]);
        }
        for (unsigned i = 0; i < Log::RECORDS_COUNT + 1; i++){
            Log::records[i] = nullptr;
        }
        Log::replacement_status = true;
        Log::record_number = 0;
    }

    void TestEmptyLog() {
        status = TBB_malloc_replacement_log(&func_replacement_log);

        REQUIRE_MESSAGE(status == -1, "Status is true, but log is empty");
        REQUIRE_MESSAGE(*func_replacement_log == nullptr, "Log must be empty");
    }

    void TestLogOverload() {
        for (int i = 0; i < 1000; i++)
            Log::record(funcInfo, "opcode string", true);

        status = TBB_malloc_replacement_log(&func_replacement_log);
        // Find last record
        for (; *(func_replacement_log + 1) != 0; func_replacement_log++) {}

        std::string last_line(*func_replacement_log);
        REQUIRE_MESSAGE(status == 0, "False status, but all functions found");
        REQUIRE_MESSAGE(last_line.compare("Log was truncated.") == 0, "Log overflow was not handled");

        // Change status
        Log::record(funcInfo, "opcode string", false);
        status = TBB_malloc_replacement_log(nullptr);
        REQUIRE_MESSAGE(status == -1, "Status is true, but we have false search case");

        LogCleanup();
    }

    void TestFalseSearchCase() {
        Log::record(funcInfo, "opcode string", false);
        std::string expected_line = "Fail: "+ std::string(funcInfo.funcName) + " (" +
                         std::string(funcInfo.dllName) + "), byte pattern: <opcode string>";

        status = TBB_malloc_replacement_log(&func_replacement_log);

        REQUIRE_MESSAGE(expected_line.compare(*func_replacement_log) == 0, "Wrong last string contnent");
        REQUIRE_MESSAGE(status == -1, "Status is true, but we have false search case");
        LogCleanup();
    }

    void TestWrongFunctionInDll(){
        HMODULE ucrtbase_handle = GetModuleHandle("ucrtbase.dll");
        if (ucrtbase_handle) {
            IsPrologueKnown("ucrtbase.dll", "fake_function", nullptr, ucrtbase_handle);
            std::string expected_line = "Fail: fake_function (ucrtbase.dll), byte pattern: <unknown>";

            status = TBB_malloc_replacement_log(&func_replacement_log);

            REQUIRE_MESSAGE(expected_line.compare(*func_replacement_log) == 0, "Wrong last string contnent");
            REQUIRE_MESSAGE(status == -1, "Status is true, but we have false search case");
            LogCleanup();
        } else {
            INFO("Cannot found ucrtbase.dll on system, test skipped!\n");
        }
    }
}

void TesFunctionReplacementLog() {
    using namespace FunctionReplacement;
    // Do not reorder the test cases
    TestEmptyLog();
    TestLogOverload();
    TestFalseSearchCase();
    TestWrongFunctionInDll();
}

#endif /*!__TBB_WIN8UI_SUPPORT && defined(_WIN32)*/

#include <cmath> // pow function

// Huge objects cache: Size = MinSize * (2 ^ (Index / StepFactor) formula gives value for the bin size,
// but it is not matched with our sizeToIdx approximation algorithm, where step sizes between major
// (power of 2) sizes are equal. Used internally for the test. Static cast to avoid warnings.
inline size_t hocIdxToSizeFormula(int idx) {
    return static_cast<size_t>(float(rml::internal::LargeObjectCache::maxLargeSize) *
        pow(2, float(idx) / float(rml::internal::LargeObjectCache::HugeBSProps::StepFactor)));
}
// Large objects cache arithmetic progression
inline size_t locIdxToSizeFormula(int idx) {
    return rml::internal::LargeObjectCache::LargeBSProps::MinSize +
        (idx * rml::internal::LargeObjectCache::LargeBSProps::CacheStep);
}

template <typename CacheType>
void TestLOCacheBinsConverterImpl(int idx, size_t checkingSize) {
    size_t alignedSize = CacheType::alignToBin(checkingSize);
    REQUIRE_MESSAGE(alignedSize >= checkingSize, "Size is not correctly aligned");
    int calcIdx = CacheType::sizeToIdx(alignedSize);
    REQUIRE_MESSAGE(calcIdx == idx, "Index from size calculated not correctly");
}

void TestLOCacheBinsConverter(){
    typedef rml::internal::LargeObjectCache::LargeCacheType LargeCacheType;
    typedef rml::internal::LargeObjectCache::HugeCacheType HugeCacheType;

    size_t checkingSize = 0;
    for (int idx = 0; idx < LargeCacheType::numBins; idx++) {
        checkingSize = locIdxToSizeFormula(idx);
        TestLOCacheBinsConverterImpl<LargeCacheType>(idx, checkingSize);
    }
    for (int idx = 0; idx < HugeCacheType::numBins; idx++) {
        checkingSize = hocIdxToSizeFormula(idx);
        TestLOCacheBinsConverterImpl<HugeCacheType>(idx, checkingSize);
    }
}

struct HOThresholdTester {
    LargeObjectCache* loc;
    size_t hugeSize;

    static const size_t sieveSize = LargeObjectCache::defaultMaxHugeSize;
    // Sieve starts from 64MB (24-th cache bin), enough to check 4 bins radius range
    // for decent memory consumption (especially for 32-bit arch)
    static const int MIN_BIN_IDX = 21;
    static const int MAX_BIN_IDX = 27;

    enum CleanupType {
        NO_CLEANUP,
        REGULAR_CLEANUP,
        HARD_CLEANUP
    };

    void populateCache() {
        LargeMemoryBlock* loArray[MAX_BIN_IDX - MIN_BIN_IDX];
        // To avoid backend::softCacheCleanup consequences (cleanup by isLOCToolarge),
        // firstly allocate all objects and then cache them at once.
        // Morevover, just because first cache item will still be dropped from cache because of the lack of history,
        // redo allocation 2 times.
        for (int idx = MIN_BIN_IDX; idx < MAX_BIN_IDX; ++idx) {
            size_t allocationSize = alignedSizeFromIdx(idx);
            int localIdx = idx - MIN_BIN_IDX;
            loArray[localIdx] = defaultMemPool->extMemPool.mallocLargeObject(defaultMemPool, allocationSize);
            REQUIRE_MESSAGE(loArray[localIdx], "Large object was not allocated.");
            loc->put(loArray[localIdx]);
            loArray[localIdx] = defaultMemPool->extMemPool.mallocLargeObject(defaultMemPool, allocationSize);
        }
        for (int idx = MIN_BIN_IDX; idx < MAX_BIN_IDX; ++idx) {
            loc->put(loArray[idx - MIN_BIN_IDX]);
        }
    }
    void clean(bool all) {
        if (all) {
            // Should avoid any threshold and clean all bins
            loc->cleanAll();
        } else {
            // Regular cleanup should do nothing for bins above threshold. Decreasing option used
            // for the test to be sure that all objects below defaultMaxHugeSize (sieveSize) were cleaned
            loc->regularCleanup();
            loc->decreasingCleanup();
        }
    }
    void check(CleanupType type) {
        for (int idx = MIN_BIN_IDX; idx < MAX_BIN_IDX; ++idx) {
            size_t objectSize = alignedSizeFromIdx(idx);
            // Cache object below sieve threshold and above huge object threshold should be cached
            // (other should be sieved). Unless all cache is dropped. Regular cleanup drops object only below sieve size.
            if (type == NO_CLEANUP && sizeInCacheRange(objectSize)) {
                REQUIRE_MESSAGE(objectInCacheBin(idx, objectSize), "Object was released from cache, it shouldn't.");
            } else if (type == REGULAR_CLEANUP && (objectSize >= hugeSize)) {
                REQUIRE_MESSAGE(objectInCacheBin(idx, objectSize), "Object was released from cache, it shouldn't.");
            } else { // HARD_CLEANUP
                REQUIRE_MESSAGE(cacheBinEmpty(idx), "Object is still cached.");
            }
        }
    }

private:
    bool cacheBinEmpty(int idx) {
        return (loc->hugeCache.bin[idx].cachedSize.load(std::memory_order_relaxed) == 0 && loc->hugeCache.bin[idx].get() == nullptr);
    }
    bool objectInCacheBin(int idx, size_t size) {
        return (loc->hugeCache.bin[idx].cachedSize.load(std::memory_order_relaxed) != 0 &&
            loc->hugeCache.bin[idx].cachedSize.load(std::memory_order_relaxed) % size == 0);
    }
    bool sizeInCacheRange(size_t size) {
        return size <= sieveSize || size >= hugeSize;
    }
    size_t alignedSizeFromIdx(int idx) {
        return rml::internal::LargeObjectCache::alignToBin(hocIdxToSizeFormula(idx));
    }
};

// TBBMALLOC_SET_HUGE_OBJECT_THRESHOLD value should be set before the test,
// through scalable API or env variable
void TestHugeSizeThresholdImpl(LargeObjectCache* loc, size_t hugeSize, bool fullTesting) {
    HOThresholdTester test = {loc, hugeSize};
    test.populateCache();
    // Check the default sieve value
    test.check(HOThresholdTester::NO_CLEANUP);

    if(fullTesting) {
        // Check that objects above threshold stay in cache after regular cleanup
        test.clean(/*all*/false);
        test.check(HOThresholdTester::REGULAR_CLEANUP);
    }
    // Check that all objects dropped from cache after hard cleanup (ignore huge obects threshold)
    test.clean(/*all*/true);
    test.check(HOThresholdTester::HARD_CLEANUP);
    // Restore previous settings
    loc->setHugeSizeThreshold(LargeObjectCache::maxHugeSize);
    loc->reset();
}

/*
 *  Test for default huge size and behaviour when huge object settings defined
 */
void TestHugeSizeThreshold() {
    // Clean up if something was allocated before the test and reset cache state
    scalable_allocation_command(TBBMALLOC_CLEAN_ALL_BUFFERS, nullptr);
    LargeObjectCache* loc = &defaultMemPool->extMemPool.loc;
    // Restore default settings just in case
    loc->setHugeSizeThreshold(LargeObjectCache::maxHugeSize);
    loc->reset();
    // Firstly check default huge size value (with max huge object threshold).
    // Everything that more then this value should be released to OS without caching.
    TestHugeSizeThresholdImpl(loc, loc->hugeSizeThreshold, false);
    // Then set huge object threshold.
    // All objects with sizes after threshold will be released only after the hard cleanup.
#if !__TBB_WIN8UI_SUPPORT
    // Unit testing for environment variable
    utils::SetEnv("TBB_MALLOC_SET_HUGE_SIZE_THRESHOLD","67108864");
    // Large object cache reads threshold environment during initialization.
    // Reset the value before the test.
    loc->hugeSizeThreshold = 0;
    // Reset logical time to prevent regular cleanup
    loc->cacheCurrTime = 0;
    loc->init(&defaultMemPool->extMemPool);
    TestHugeSizeThresholdImpl(loc, 64 * MByte, true);
#endif
    // Unit testing for scalable_allocation_command
    scalable_allocation_mode(TBBMALLOC_SET_HUGE_SIZE_THRESHOLD, 56 * MByte);
    TestHugeSizeThresholdImpl(loc, 56 * MByte, true);
    // Verify that objects whose sizes align to maxHugeSize are not cached.
    size_t sz = LargeObjectCache::maxHugeSize;
    size_t aligned_sz = LargeObjectCache::alignToBin(sz);
    REQUIRE_MESSAGE(sz == aligned_sz, "maxHugeSize should be aligned.");
    REQUIRE_MESSAGE(!loc->sizeInCacheRange(sz), "Upper bound sized object shouldn't be cached.");
    REQUIRE_MESSAGE(loc->get(sz) == nullptr, "Upper bound sized object shouldn't be cached.");
}

//! \brief \ref error_guessing
TEST_CASE("Main test case") {
    scalable_allocation_mode(USE_HUGE_PAGES, 0);
#if !__TBB_WIN8UI_SUPPORT
    utils::SetEnv("TBB_MALLOC_USE_HUGE_PAGES","yes");
#endif
    checkNoHugePages();
    // backreference requires that initialization was done
    if(!isMallocInitialized()) doInitialization();
    checkNoHugePages();
    // to succeed, leak detection must be the 1st memory-intensive test
    TestBackRef();
    TestCleanAllBuffers<4*1024>();
    TestCleanAllBuffers<16*1024>();
    TestCleanThreadBuffers();
    TestPools();
    TestBackend();

#if MALLOC_CHECK_RECURSION
    for( int p=MaxThread; p>=MinThread; --p ) {
        TestStartupAlloc::initBarrier( p );
        utils::NativeParallelFor( p, TestStartupAlloc() );
        REQUIRE_MESSAGE(!firstStartupBlock, "Startup heap memory leak detected");
    }
#endif
    TestLargeObjectCache();
    TestObjectRecognition();
    TestBitMask();
    TestHeapLimit();
    TestLOC();
    TestSlabAlignment();
}

//! \brief \ref error_guessing
TEST_CASE("Decreasing reallocation") {
    if (!isMallocInitialized()) doInitialization();
    TestReallocDecreasing();
}

//! \brief \ref error_guessing
TEST_CASE("Large object cache bins converter") {
    if (!isMallocInitialized()) doInitialization();
    TestLOCacheBinsConverter();
}

//! \brief \ref error_guessing
TEST_CASE("Huge size threshold settings") {
    if (!isMallocInitialized()) doInitialization();
    TestHugeSizeThreshold();
}

#if __unix__
//! \brief \ref error_guessing
TEST_CASE("Transparent huge pages") {
    if (utils::isTHPEnabledOnMachine()) {
        if (!isMallocInitialized()) doInitialization();
        TestTHP();
    } else {
        INFO("Transparent Huge Pages is not supported on the system - skipped the test\n");
    }
}
#endif

#if !__TBB_WIN8UI_SUPPORT && defined(_WIN32)
//! \brief \ref error_guessing
TEST_CASE("Function replacement log") {
    TesFunctionReplacementLog();
}
#endif
