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

#ifndef __TBB_tbbmalloc_internal_H
#define __TBB_tbbmalloc_internal_H

#include "TypeDefinitions.h" /* Also includes customization layer Customize.h */

#if USE_PTHREAD
    // Some pthreads documentation says that <pthreads.h> must be first header.
    #include <pthread.h>
    typedef pthread_key_t tls_key_t;
#elif USE_WINTHREAD
    #include <windows.h>
    typedef DWORD tls_key_t;
#else
    #error Must define USE_PTHREAD or USE_WINTHREAD
#endif

#include <atomic>

// TODO: *BSD also has it
#define BACKEND_HAS_MREMAP __linux__
#define CHECK_ALLOCATION_RANGE MALLOC_DEBUG || MALLOC_ZONE_OVERLOAD_ENABLED || MALLOC_UNIXLIKE_OVERLOAD_ENABLED

#include "oneapi/tbb/detail/_config.h" // for __TBB_LIBSTDCPP_EXCEPTION_HEADERS_BROKEN
#include "oneapi/tbb/detail/_template_helpers.h"
#if __TBB_LIBSTDCPP_EXCEPTION_HEADERS_BROKEN
  #define _EXCEPTION_PTR_H /* prevents exception_ptr.h inclusion */
  #define _GLIBCXX_NESTED_EXCEPTION_H /* prevents nested_exception.h inclusion */
#endif

#include <stdio.h>
#include <stdlib.h>
#include <limits.h> // for CHAR_BIT
#include <string.h> // for memset
#if MALLOC_CHECK_RECURSION
#include <new>        /* for placement new */
#endif
#include "oneapi/tbb/scalable_allocator.h"
#include "tbbmalloc_internal_api.h"

/********* Various compile-time options        **************/

#if !__TBB_DEFINE_MIC && __TBB_MIC_NATIVE
 #error Intel(R) Many Integrated Core Compiler does not define __MIC__ anymore.
#endif

#define MALLOC_TRACE 0

#if MALLOC_TRACE
#define TRACEF(x) printf x
#else
#define TRACEF(x) ((void)0)
#endif /* MALLOC_TRACE */

#define ASSERT_TEXT nullptr

#define COLLECT_STATISTICS ( MALLOC_DEBUG && MALLOCENV_COLLECT_STATISTICS )
#ifndef USE_INTERNAL_TID
#define USE_INTERNAL_TID COLLECT_STATISTICS || MALLOC_TRACE
#endif

#include "Statistics.h"

// call yield for whitebox testing, skip in real library
#ifndef WhiteboxTestingYield
#define WhiteboxTestingYield() ((void)0)
#endif


/********* End compile-time options        **************/

namespace rml {

namespace internal {

#if __TBB_MALLOC_LOCACHE_STAT
extern intptr_t mallocCalls, cacheHits;
extern intptr_t memAllocKB, memHitKB;
#endif

//! Utility template function to prevent "unused" warnings by various compilers.
template<typename T>
void suppress_unused_warning( const T& ) {}

/********** Various global default constants ********/

/*
 * Default huge page size
 */
static const size_t HUGE_PAGE_SIZE = 2 * 1024 * 1024;

/********** End of global default constatns *********/

/********** Various numeric parameters controlling allocations ********/

/*
 * slabSize - the size of a block for allocation of small objects,
 * it must be larger than maxSegregatedObjectSize.
 */
const uintptr_t slabSize = 16*1024;

/*
 * Large blocks cache cleanup frequency.
 * It should be power of 2 for the fast checking.
 */
const unsigned cacheCleanupFreq = 256;

/*
 * Alignment of large (>= minLargeObjectSize) objects.
 */
const size_t largeObjectAlignment = estimatedCacheLineSize;

/*
 * This number of bins in the TLS that leads to blocks that we can allocate in.
 */
const uint32_t numBlockBinLimit = 31;

/********** End of numeric parameters controlling allocations *********/

class BlockI;
class Block;
struct LargeMemoryBlock;
struct ExtMemoryPool;
struct MemRegion;
class FreeBlock;
class TLSData;
class Backend;
class MemoryPool;
struct CacheBinOperation;
extern const uint32_t minLargeObjectSize;

enum DecreaseOrIncrease {
    decrease, increase
};

class TLSKey {
    tls_key_t TLS_pointer_key;
public:
    bool init();
    bool destroy();
    TLSData* getThreadMallocTLS() const;
    void setThreadMallocTLS( TLSData * newvalue );
    TLSData* createTLS(MemoryPool *memPool, Backend *backend);
};

template<typename Arg, typename Compare>
inline void AtomicUpdate(std::atomic<Arg>& location, Arg newVal, const Compare &cmp)
{
    static_assert(sizeof(Arg) == sizeof(intptr_t), "Type of argument must match AtomicCompareExchange type.");
    Arg old = location.load(std::memory_order_acquire);
    for (; cmp(old, newVal); ) {
        if (location.compare_exchange_strong(old, newVal))
            break;
        // TODO: do we need backoff after unsuccessful CAS?
        //old = val;
    }
}

// TODO: make BitMaskBasic more general
// TODO: check that BitMaskBasic is not used for synchronization
// (currently, it fits BitMaskMin well, but not as suitable for BitMaskMax)
template<unsigned NUM>
class BitMaskBasic {
    static const unsigned SZ = (NUM-1)/(CHAR_BIT*sizeof(uintptr_t))+1;
    static const unsigned WORD_LEN = CHAR_BIT*sizeof(uintptr_t);

    std::atomic<uintptr_t> mask[SZ];

protected:
    void set(size_t idx, bool val) {
        MALLOC_ASSERT(idx<NUM, ASSERT_TEXT);

        size_t i = idx / WORD_LEN;
        int pos = WORD_LEN - idx % WORD_LEN - 1;
        if (val) {
            mask[i].fetch_or(1ULL << pos);
        } else {
            mask[i].fetch_and(~(1ULL << pos));
        }
    }
    int getMinTrue(unsigned startIdx) const {
        unsigned idx = startIdx / WORD_LEN;
        int pos;

        if (startIdx % WORD_LEN) {
            // only interested in part of a word, clear bits before startIdx
            pos = WORD_LEN - startIdx % WORD_LEN;
            uintptr_t actualMask = mask[idx].load(std::memory_order_relaxed) & (((uintptr_t)1<<pos) - 1);
            idx++;
            if (-1 != (pos = BitScanRev(actualMask)))
                return idx*WORD_LEN - pos - 1;
        }

        while (idx<SZ)
            if (-1 != (pos = BitScanRev(mask[idx++].load(std::memory_order_relaxed))))
                return idx*WORD_LEN - pos - 1;
        return -1;
    }
public:
    void reset() { for (unsigned i=0; i<SZ; i++) mask[i].store(0, std::memory_order_relaxed); }
};

template<unsigned NUM>
class BitMaskMin : public BitMaskBasic<NUM> {
public:
    void set(size_t idx, bool val) { BitMaskBasic<NUM>::set(idx, val); }
    int getMinTrue(unsigned startIdx) const {
        return BitMaskBasic<NUM>::getMinTrue(startIdx);
    }
};

template<unsigned NUM>
class BitMaskMax : public BitMaskBasic<NUM> {
public:
    void set(size_t idx, bool val) {
        BitMaskBasic<NUM>::set(NUM - 1 - idx, val);
    }
    int getMaxTrue(unsigned startIdx) const {
        int p = BitMaskBasic<NUM>::getMinTrue(NUM-startIdx-1);
        return -1==p? -1 : (int)NUM - 1 - p;
    }
};


// The part of thread-specific data that can be modified by other threads.
// Such modifications must be protected by AllLocalCaches::listLock.
struct TLSRemote {
    TLSRemote *next,
              *prev;
};

// The list of all thread-local data; supporting cleanup of thread caches
class AllLocalCaches {
    TLSRemote  *head;
    MallocMutex listLock; // protects operations in the list
public:
    void registerThread(TLSRemote *tls);
    void unregisterThread(TLSRemote *tls);
    bool cleanup(bool cleanOnlyUnused);
    void markUnused();
    void reset() { head = nullptr; }
};

class LifoList {
public:
    inline LifoList();
    inline void push(Block *block);
    inline Block *pop();
    inline Block *grab();

private:
    std::atomic<Block*> top;
    MallocMutex lock;
};

/*
 * When a block that is not completely free is returned for reuse by other threads
 * this is where the block goes.
 *
 * LifoList assumes zero initialization; so below its constructors are omitted,
 * to avoid linking with C++ libraries on Linux.
 */

class OrphanedBlocks {
    LifoList bins[numBlockBinLimit];
public:
    Block *get(TLSData *tls, unsigned int size);
    void put(intptr_t binTag, Block *block);
    void reset();
    bool cleanup(Backend* backend);
};

/* Large objects entities */
#include "large_objects.h"

// select index size for BackRefMain based on word size: default is uint32_t,
// uint16_t for 32-bit platforms
template<bool>
struct MainIndexSelect {
    typedef uint32_t main_type;
};

template<>
struct MainIndexSelect<false> {
    typedef uint16_t main_type;
};

class BackRefIdx { // composite index to backreference array
public:
    typedef MainIndexSelect<4 < sizeof(uintptr_t)>::main_type main_t;
private:
    static const main_t invalid = ~main_t(0);
    main_t main;      // index in BackRefMain
    uint16_t largeObj:1;  // is this object "large"?
    uint16_t offset  :15; // offset from beginning of BackRefBlock
public:
    BackRefIdx() : main(invalid), largeObj(0), offset(0) {}
    bool isInvalid() const { return main == invalid; }
    bool isLargeObject() const { return largeObj; }
    main_t getMain() const { return main; }
    uint16_t getOffset() const { return offset; }

#if __TBB_USE_THREAD_SANITIZER
    friend
    __attribute__((no_sanitize("thread")))
     BackRefIdx dereference(const BackRefIdx* ptr) {
        BackRefIdx idx;
        idx.main = ptr->main;
        idx.largeObj = ptr->largeObj;
        idx.offset = ptr->offset;
        return idx;
    }
#else
    friend
    BackRefIdx dereference(const BackRefIdx* ptr) {
        return *ptr;
    }
#endif

    // only newBackRef can modify BackRefIdx
    static BackRefIdx newBackRef(bool largeObj);
};

// Block header is used during block coalescing
// and must be preserved in used blocks.
class BlockI {
#if __clang__
    #pragma clang diagnostic push
    #pragma clang diagnostic ignored "-Wunused-private-field"
#endif
    intptr_t     blockState[2];
#if __clang__
    #pragma clang diagnostic pop // "-Wunused-private-field"
#endif
};

struct LargeMemoryBlock : public BlockI {
    MemoryPool       *pool;          // owner pool
    LargeMemoryBlock *next,          // ptrs in list of cached blocks
                     *prev,
    // 2-linked list of pool's large objects
    // Used to destroy backrefs on pool destroy (backrefs are global)
    // and for object releasing during pool reset.
                     *gPrev,
                     *gNext;
    uintptr_t         age;           // age of block while in cache
    size_t            objectSize;    // the size requested by a client
    size_t            unalignedSize; // the size requested from backend
    BackRefIdx        backRefIdx;    // cached here, used copy is in LargeObjectHdr
};

// Classes and methods for backend.cpp
#include "backend.h"

// An TBB allocator mode that can be controlled by user
// via API/environment variable. Must be placed in zero-initialized memory.
// External synchronization assumed.
// TODO: TBB_VERSION support
class AllocControlledMode {
    intptr_t val;
    bool     setDone;

public:
    intptr_t get() const {
        MALLOC_ASSERT(setDone, ASSERT_TEXT);
        return val;
    }

    // Note: set() can be called before init()
    void set(intptr_t newVal) {
        val = newVal;
        setDone = true;
    }

    bool ready() const {
        return setDone;
    }

    // envName - environment variable to get controlled mode
    void initReadEnv(const char *envName, intptr_t defaultVal) {
        if (!setDone) {
            // unreferenced formal parameter warning
            tbb::detail::suppress_unused_warning(envName);
#if !__TBB_WIN8UI_SUPPORT
        // TODO: use strtol to get the actual value of the envirable
            const char *envVal = getenv(envName);
            if (envVal && !strcmp(envVal, "1"))
                val = 1;
            else
#endif
                val = defaultVal;
            setDone = true;
        }
    }
};

// Page type to be used inside MapMemory.
// Regular (4KB aligned), Huge and Transparent Huge Pages (2MB aligned).
enum PageType {
    REGULAR = 0,
    PREALLOCATED_HUGE_PAGE,
    TRANSPARENT_HUGE_PAGE
};

// init() and printStatus() is called only under global initialization lock.
// Race is possible between registerAllocation() and registerReleasing(),
// harm is that up to single huge page releasing is missed (because failure
// to get huge page is registered only 1st time), that is negligible.
// setMode is also can be called concurrently.
// Object must reside in zero-initialized memory
// TODO: can we check for huge page presence during every 10th mmap() call
// in case huge page is released by another process?
class HugePagesStatus {
private:
    AllocControlledMode requestedMode; // changed only by user
                                       // to keep enabled and requestedMode consistent
    MallocMutex setModeLock;
    size_t      pageSize;
    std::atomic<intptr_t> needActualStatusPrint;

    static void doPrintStatus(bool state, const char *stateName) {
        // Under macOS* fprintf/snprintf acquires an internal lock, so when
        // 1st allocation is done under the lock, we got a deadlock.
        // Do not use fprintf etc during initialization.
        fputs("TBBmalloc: huge pages\t", stderr);
        if (!state)
            fputs("not ", stderr);
        fputs(stateName, stderr);
        fputs("\n", stderr);
    }

    void parseSystemMemInfo() {
        bool hpAvailable  = false;
        bool thpAvailable = false;
        long long hugePageSize = -1;

#if __unix__
        // Check huge pages existence
        long long meminfoHugePagesTotal = 0;

        parseFileItem meminfoItems[] = {
            // Parse system huge page size
            { "Hugepagesize: %lld kB", hugePageSize },
            // Check if there are preallocated huge pages on the system
            // https://www.kernel.org/doc/Documentation/vm/hugetlbpage.txt
            { "HugePages_Total: %lld", meminfoHugePagesTotal } };

        parseFile</*BUFF_SIZE=*/100>("/proc/meminfo", meminfoItems);

        // Double check another system information regarding preallocated
        // huge pages if there are no information in /proc/meminfo
        long long vmHugePagesTotal = 0;

        parseFileItem vmItem[] = { { "%lld", vmHugePagesTotal } };

        // We parse a counter number, it can't be huge
        parseFile</*BUFF_SIZE=*/100>("/proc/sys/vm/nr_hugepages", vmItem);

        if (hugePageSize > -1 && (meminfoHugePagesTotal > 0 || vmHugePagesTotal > 0)) {
            MALLOC_ASSERT(hugePageSize != 0, "Huge Page size can't be zero if we found preallocated.");

            // Any non zero value clearly states that there are preallocated
            // huge pages on the system
            hpAvailable = true;
        }

        // Check if there is transparent huge pages support on the system
        long long thpPresent = 'n';
        parseFileItem thpItem[] = { { "[alwa%cs] madvise never\n", thpPresent } };
        parseFile</*BUFF_SIZE=*/100>("/sys/kernel/mm/transparent_hugepage/enabled", thpItem);

        if (hugePageSize > -1 && thpPresent == 'y') {
            MALLOC_ASSERT(hugePageSize != 0, "Huge Page size can't be zero if we found thp existence.");
            thpAvailable = true;
        }
#endif
        MALLOC_ASSERT(!pageSize, "Huge page size can't be set twice. Double initialization.");

        // Initialize object variables
        pageSize       = hugePageSize * 1024; // was read in KB from meminfo
        isHPAvailable  = hpAvailable;
        isTHPAvailable = thpAvailable;
    }

public:

    // System information
    bool isHPAvailable;
    bool isTHPAvailable;

    // User defined value
    bool isEnabled;

    void init() {
        parseSystemMemInfo();
        MallocMutex::scoped_lock lock(setModeLock);
        requestedMode.initReadEnv("TBB_MALLOC_USE_HUGE_PAGES", 0);
        isEnabled = (isHPAvailable || isTHPAvailable) && requestedMode.get();
    }

    // Could be set from user code at any place.
    // If we didn't call init() at this place, isEnabled will be false
    void setMode(intptr_t newVal) {
        MallocMutex::scoped_lock lock(setModeLock);
        requestedMode.set(newVal);
        isEnabled = (isHPAvailable || isTHPAvailable) && newVal;
    }

    void reset() {
        needActualStatusPrint.store(0, std::memory_order_relaxed);
        pageSize = 0;
        isEnabled = isHPAvailable = isTHPAvailable = false;
    }

    // If memory mapping size is a multiple of huge page size, some OS kernels
    // can use huge pages transparently. Use this when huge pages are requested.
    size_t getGranularity() const {
        if (requestedMode.ready())
            return requestedMode.get() ? pageSize : 0;
        else
            return HUGE_PAGE_SIZE; // the mode is not yet known; assume typical 2MB huge pages
    }

    void printStatus() {
        doPrintStatus(requestedMode.get(), "requested");
        if (requestedMode.get()) { // report actual status iff requested
            if (pageSize)
                needActualStatusPrint.store(1, std::memory_order_release);
            else
                doPrintStatus(/*state=*/false, "available");
        }
    }
};

class AllLargeBlocksList {
    MallocMutex       largeObjLock;
    LargeMemoryBlock *loHead;
public:
    void add(LargeMemoryBlock *lmb);
    void remove(LargeMemoryBlock *lmb);
    template<bool poolDestroy> void releaseAll(Backend *backend);
};

struct ExtMemoryPool {
    Backend           backend;
    LargeObjectCache  loc;
    AllLocalCaches    allLocalCaches;
    OrphanedBlocks    orphanedBlocks;

    intptr_t          poolId;
    // To find all large objects. Used during user pool destruction,
    // to release all backreferences in large blocks (slab blocks do not have them).
    AllLargeBlocksList lmbList;
    // Callbacks to be used instead of MapMemory/UnmapMemory.
    rawAllocType      rawAlloc;
    rawFreeType       rawFree;
    size_t            granularity;
    bool              keepAllMemory,
                      delayRegsReleasing,
    // TODO: implements fixedPool with calling rawFree on destruction
                      fixedPool;
    TLSKey            tlsPointerKey;  // per-pool TLS key

    bool init(intptr_t poolId, rawAllocType rawAlloc, rawFreeType rawFree,
              size_t granularity, bool keepAllMemory, bool fixedPool);
    bool initTLS();

    // i.e., not system default pool for scalable_malloc/scalable_free
    bool userPool() const { return rawAlloc; }

     // true if something has been released
    bool softCachesCleanup();
    bool releaseAllLocalCaches();
    bool hardCachesCleanup();
    void *remap(void *ptr, size_t oldSize, size_t newSize, size_t alignment);
    bool reset() {
        loc.reset();
        allLocalCaches.reset();
        orphanedBlocks.reset();
        bool ret = tlsPointerKey.destroy();
        backend.reset();
        return ret;
    }
    bool destroy() {
        MALLOC_ASSERT(isPoolValid(),
                      "Possible double pool_destroy or heap corruption");
        if (!userPool()) {
            loc.reset();
            allLocalCaches.reset();
        }
        // pthread_key_dtors must be disabled before memory unmapping
        // TODO: race-free solution
        bool ret = tlsPointerKey.destroy();
        if (rawFree || !userPool())
            ret &= backend.destroy();
        // pool is not valid after this point
        granularity = 0;
        return ret;
    }
    void delayRegionsReleasing(bool mode) { delayRegsReleasing = mode; }
    inline bool regionsAreReleaseable() const;

    LargeMemoryBlock *mallocLargeObject(MemoryPool *pool, size_t allocationSize);
    void freeLargeObject(LargeMemoryBlock *lmb);
    void freeLargeObjectList(LargeMemoryBlock *head);
#if MALLOC_DEBUG
    // use granulatity as marker for pool validity
    bool isPoolValid() const { return granularity; }
#endif
};

inline bool Backend::inUserPool() const { return extMemPool->userPool(); }

struct LargeObjectHdr {
    LargeMemoryBlock *memoryBlock;
    /* Backreference points to LargeObjectHdr.
       Duplicated in LargeMemoryBlock to reuse in subsequent allocations. */
    BackRefIdx       backRefIdx;
};

struct FreeObject {
    FreeObject  *next;
};


/******* A helper class to support overriding malloc with scalable_malloc *******/
#if MALLOC_CHECK_RECURSION

class RecursiveMallocCallProtector {
    // pointer to an automatic data of holding thread
    static std::atomic<void*> autoObjPtr;
    static MallocMutex rmc_mutex;
    static std::atomic<pthread_t> owner_thread;
/* Under FreeBSD 8.0 1st call to any pthread function including pthread_self
   leads to pthread initialization, that causes malloc calls. As 1st usage of
   RecursiveMallocCallProtector can be before pthread initialized, pthread calls
   can't be used in 1st instance of RecursiveMallocCallProtector.
   RecursiveMallocCallProtector is used 1st time in checkInitialization(),
   so there is a guarantee that on 2nd usage pthread is initialized.
   No such situation observed with other supported OSes.
 */
#if __FreeBSD__
    static bool        canUsePthread;
#else
    static const bool  canUsePthread = true;
#endif
/*
  The variable modified in checkInitialization,
  so can be read without memory barriers.
 */
    static bool mallocRecursionDetected;

    MallocMutex::scoped_lock* lock_acquired;
    char scoped_lock_space[sizeof(MallocMutex::scoped_lock)+1];
    
public:

    RecursiveMallocCallProtector() : lock_acquired(nullptr) {
        lock_acquired = new (scoped_lock_space) MallocMutex::scoped_lock( rmc_mutex );
        if (canUsePthread)
            owner_thread.store(pthread_self(), std::memory_order_relaxed);
        autoObjPtr.store(&scoped_lock_space, std::memory_order_relaxed);
    }
    ~RecursiveMallocCallProtector() {
        if (lock_acquired) {
            autoObjPtr.store(nullptr, std::memory_order_relaxed);
            lock_acquired->~scoped_lock();
        }
    }
    static bool sameThreadActive() {
        if (!autoObjPtr.load(std::memory_order_relaxed)) // fast path
            return false;
        // Some thread has an active recursive call protector; check if the current one.
        // Exact pthread_self based test
        if (canUsePthread) {
            if (pthread_equal( owner_thread.load(std::memory_order_relaxed), pthread_self() )) {
                mallocRecursionDetected = true;
                return true;
            } else
                return false;
        }
        // inexact stack size based test
        const uintptr_t threadStackSz = 2*1024*1024;
        int dummy;

        uintptr_t xi = (uintptr_t)autoObjPtr.load(std::memory_order_relaxed), yi = (uintptr_t)&dummy;
        uintptr_t diffPtr = xi > yi ? xi - yi : yi - xi;

        return diffPtr < threadStackSz;
    }

/* The function is called on 1st scalable_malloc call to check if malloc calls
   scalable_malloc (nested call must set mallocRecursionDetected). */
    static void detectNaiveOverload() {
        if (!malloc_proxy) {
#if __FreeBSD__
/* If !canUsePthread, we can't call pthread_self() before, but now pthread
   is already on, so can do it. */
            if (!canUsePthread) {
                canUsePthread = true;
                owner_thread.store(pthread_self(), std::memory_order_relaxed);
            }
#endif
            free(malloc(1));
        }
    }
};

#else

class RecursiveMallocCallProtector {
public:
    RecursiveMallocCallProtector() {}
    ~RecursiveMallocCallProtector() {}
};

#endif  /* MALLOC_CHECK_RECURSION */

unsigned int getThreadId();

bool initBackRefMain(Backend *backend);
void destroyBackRefMain(Backend *backend);
void removeBackRef(BackRefIdx backRefIdx);
void setBackRef(BackRefIdx backRefIdx, void *newPtr);
void *getBackRef(BackRefIdx backRefIdx);

} // namespace internal
} // namespace rml

#endif // __TBB_tbbmalloc_internal_H
