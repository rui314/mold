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

#ifndef __TBB_tbbmalloc_internal_H
    #error tbbmalloc_internal.h must be included at this point
#endif

#ifndef __TBB_backend_H
#define __TBB_backend_H

// Included from namespace rml::internal

// global state of blocks currently in processing
class BackendSync {
    // Class instances should reside in zero-initialized memory!
    // The number of blocks currently removed from a bin and not returned back
    std::atomic<intptr_t> inFlyBlocks;        // to another
    std::atomic<intptr_t> binsModifications;  // incremented on every bin modification
    Backend *backend;
public:
    void init(Backend *b) { backend = b; }
    void blockConsumed() { inFlyBlocks++; }
    void binsModified() { binsModifications++; }
    void blockReleased() {
#if __TBB_MALLOC_BACKEND_STAT
        MALLOC_ITT_SYNC_RELEASING(&inFlyBlocks);
#endif
        binsModifications++;
        intptr_t prev = inFlyBlocks.fetch_sub(1);
        MALLOC_ASSERT(prev > 0, ASSERT_TEXT);
        suppress_unused_warning(prev);
    }
    intptr_t getNumOfMods() const { return binsModifications.load(std::memory_order_acquire); }
    // return true if need re-do the blocks search
    inline bool waitTillBlockReleased(intptr_t startModifiedCnt);
};

class CoalRequestQ { // queue of free blocks that coalescing was delayed
private:
    std::atomic<FreeBlock*> blocksToFree;
    BackendSync *bkndSync;
    // counted blocks in blocksToFree and that are leaved blocksToFree
    // and still in active coalescing
    std::atomic<intptr_t> inFlyBlocks;
public:
    void init(BackendSync *bSync) { bkndSync = bSync; }
    FreeBlock *getAll(); // return current list of blocks and make queue empty
    void putBlock(FreeBlock *fBlock);
    inline void blockWasProcessed();
    intptr_t blocksInFly() const { return inFlyBlocks.load(std::memory_order_acquire); }
};

class MemExtendingSema {
    std::atomic<intptr_t>    active;
public:
    bool wait() {
        bool rescanBins = false;
        // up to 3 threads can add more memory from OS simultaneously,
        // rest of threads have to wait
        intptr_t prevCnt = active.load(std::memory_order_acquire);
        for (;;) {
            if (prevCnt < 3) {
                if (active.compare_exchange_strong(prevCnt, prevCnt + 1)) {
                    break;
                }
            } else {
                SpinWaitWhileEq(active, prevCnt);
                rescanBins = true;
                break;
            }
        }
        return rescanBins;
    }
    void signal() { active.fetch_sub(1); }
};

enum MemRegionType {
    // The region holds only slabs
    MEMREG_SLAB_BLOCKS = 0,
    // The region can hold several large object blocks
    MEMREG_LARGE_BLOCKS,
    // The region holds only one block with a requested size
    MEMREG_ONE_BLOCK
};

class MemRegionList {
    MallocMutex regionListLock;
public:
    MemRegion  *head;
    void add(MemRegion *r);
    void remove(MemRegion *r);
    int reportStat(FILE *f);
};

class Backend {
private:
/* Blocks in range [minBinnedSize; getMaxBinnedSize()] are kept in bins,
   one region can contains several blocks. Larger blocks are allocated directly
   and one region always contains one block.
*/
    enum {
        minBinnedSize = 8*1024UL,
        /*   If huge pages are available, maxBinned_HugePage used.
             If not, maxBinned_SmallPage is the threshold.
             TODO: use pool's granularity for upper bound setting.*/
        maxBinned_SmallPage = 1024*1024UL,
        // TODO: support other page sizes
        maxBinned_HugePage = 4*1024*1024UL
    };
    enum {
        VALID_BLOCK_IN_BIN = 1 // valid block added to bin, not returned as result
    };
public:
    // Backend bins step is the same as CacheStep for large object cache
    static const size_t   freeBinsStep = LargeObjectCache::LargeBSProps::CacheStep;
    static const unsigned freeBinsNum = (maxBinned_HugePage-minBinnedSize)/freeBinsStep + 1;

    // if previous access missed per-thread slabs pool,
    // allocate numOfSlabAllocOnMiss blocks in advance
    static const int numOfSlabAllocOnMiss = 2;

    enum {
        NO_BIN = -1,
        // special bin for blocks >= maxBinned_HugePage, blocks go to this bin
        // when pool is created with keepAllMemory policy
        // TODO: currently this bin is scanned using "1st fit", as it accumulates
        // blocks of different sizes, "best fit" is preferred in terms of fragmentation
        HUGE_BIN = freeBinsNum-1
    };

    // Bin keeps 2-linked list of free blocks. It must be 2-linked
    // because during coalescing a block it's removed from a middle of the list.
    struct Bin {
        std::atomic<FreeBlock*> head;
        FreeBlock*              tail;
        MallocMutex             tLock;

        void removeBlock(FreeBlock *fBlock);
        void reset() {
            head.store(nullptr, std::memory_order_relaxed);
            tail = nullptr;
        }
        bool empty() const { return !head.load(std::memory_order_relaxed); }

        size_t countFreeBlocks();
        size_t reportFreeBlocks(FILE *f);
        void reportStat(FILE *f);
    };

    typedef BitMaskMin<Backend::freeBinsNum> BitMaskBins;

    // array of bins supplemented with bitmask for fast finding of non-empty bins
    class IndexedBins {
        BitMaskBins bitMask;
        Bin         freeBins[Backend::freeBinsNum];
        FreeBlock *getFromBin(int binIdx, BackendSync *sync, size_t size,
                bool needAlignedBlock, bool alignedBin, bool wait, int *resLocked);
    public:
        FreeBlock *findBlock(int nativeBin, BackendSync *sync, size_t size,
                bool needAlignedBlock, bool alignedBin,int *numOfLockedBins);
        bool tryReleaseRegions(int binIdx, Backend *backend);
        void lockRemoveBlock(int binIdx, FreeBlock *fBlock);
        void addBlock(int binIdx, FreeBlock *fBlock, size_t blockSz, bool addToTail);
        bool tryAddBlock(int binIdx, FreeBlock *fBlock, bool addToTail);
        int  getMinNonemptyBin(unsigned startBin) const;
        void verify();
        void reset();
        void reportStat(FILE *f);
    };

private:
    class AdvRegionsBins {
        BitMaskBins bins;
    public:
        void registerBin(int regBin) { bins.set(regBin, 1); }
        int getMinUsedBin(int start) const { return bins.getMinTrue(start); }
        void reset() { bins.reset(); }
    };
    // auxiliary class to atomic maximum request finding
    class MaxRequestComparator {
        const Backend *backend;
    public:
        MaxRequestComparator(const Backend *be) : backend(be) {}
        inline bool operator()(size_t oldMaxReq, size_t requestSize) const;
    };

#if CHECK_ALLOCATION_RANGE
    // Keep min and max of all addresses requested from OS,
    // use it for checking memory possibly allocated by replaced allocators
    // and for debugging purposes. Valid only for default memory pool.
    class UsedAddressRange {
        static const uintptr_t ADDRESS_UPPER_BOUND = UINTPTR_MAX;

        std::atomic<uintptr_t> leftBound,
                               rightBound;
        MallocMutex mutex;
    public:
        // rightBound is zero-initialized
        void init() { leftBound.store(ADDRESS_UPPER_BOUND, std::memory_order_relaxed); }
        void registerAlloc(uintptr_t left, uintptr_t right);
        void registerFree(uintptr_t left, uintptr_t right);
        // as only left and right bounds are kept, we can return true
        // for pointer not allocated by us, if more than single region
        // was requested from OS
        bool inRange(void *ptr) const {
            const uintptr_t p = (uintptr_t)ptr;
            return leftBound.load(std::memory_order_relaxed)<=p &&
                   p<=rightBound.load(std::memory_order_relaxed);
        }
    };
#else
    class UsedAddressRange {
    public:
        void init() { }
        void registerAlloc(uintptr_t, uintptr_t) {}
        void registerFree(uintptr_t, uintptr_t) {}
        bool inRange(void *) const { return true; }
    };
#endif

    ExtMemoryPool   *extMemPool;
    // used for release every region on pool destroying
    MemRegionList    regionList;

    CoalRequestQ     coalescQ; // queue of coalescing requests
    BackendSync      bkndSync;
    // semaphore protecting adding more more memory from OS
    MemExtendingSema memExtendingSema;
    //size_t           totalMemSize,
    //                 memSoftLimit;
    std::atomic<size_t> totalMemSize;
    std::atomic<size_t> memSoftLimit;
    UsedAddressRange usedAddrRange;
    // to keep 1st allocation large than requested, keep bootstrapping status
    enum {
        bootsrapMemNotDone = 0,
        bootsrapMemInitializing,
        bootsrapMemDone
    };
    std::atomic<intptr_t> bootsrapMemStatus;
    MallocMutex      bootsrapMemStatusMutex;

    // Using of maximal observed requested size allows decrease
    // memory consumption for small requests and decrease fragmentation
    // for workloads when small and large allocation requests are mixed.
    // TODO: decrease, not only increase it
    std::atomic<size_t> maxRequestedSize;

    // register bins related to advance regions
    AdvRegionsBins advRegBins;
    // Storage for split FreeBlocks
    IndexedBins freeLargeBlockBins,
                freeSlabAlignedBins;

    // Our friends
    friend class BackendSync;

    /******************************** Backend methods ******************************/

    /*--------------------------- Coalescing functions ----------------------------*/
    void coalescAndPut(FreeBlock *fBlock, size_t blockSz, bool slabAligned);
    bool coalescAndPutList(FreeBlock *head, bool forceCoalescQDrop, bool reportBlocksProcessed);

    // Main coalescing operation
    FreeBlock *doCoalesc(FreeBlock *fBlock, MemRegion **memRegion);

    // Queue for conflicted blocks during coalescing
    bool scanCoalescQ(bool forceCoalescQDrop);
    intptr_t blocksInCoalescing() const { return coalescQ.blocksInFly(); }

    /*--------------------- FreeBlock backend accessors ---------------------------*/
    FreeBlock *genericGetBlock(int num, size_t size, bool slabAligned);
    void genericPutBlock(FreeBlock *fBlock, size_t blockSz, bool slabAligned);

    // Split the block and return remaining parts to backend if possible
    FreeBlock *splitBlock(FreeBlock *fBlock, int num, size_t size, bool isAligned, bool needAlignedBlock);

    void removeBlockFromBin(FreeBlock *fBlock);

    // TODO: combine with returnLargeObject
    void putLargeBlock(LargeMemoryBlock *lmb);

    /*------------------- Starting point for OS allocation ------------------------*/
    void requestBootstrapMem();
    FreeBlock *askMemFromOS(size_t totalReqSize, intptr_t startModifiedCnt,
                            int *lockedBinsThreshold, int numOfLockedBins,
                            bool *splittable, bool needSlabRegion);

    /*---------------------- Memory regions allocation ----------------------------*/
    FreeBlock *addNewRegion(size_t size, MemRegionType type, bool addToBin);
    void releaseRegion(MemRegion *region);

    // TODO: combine in one initMemoryRegion function
    FreeBlock *findBlockInRegion(MemRegion *region, size_t exactBlockSize);
    void startUseBlock(MemRegion *region, FreeBlock *fBlock, bool addToBin);

    /*------------------------- Raw memory accessors ------------------------------*/
    void *allocRawMem(size_t &size);
    bool freeRawMem(void *object, size_t size);

    /*------------------------------ Cleanup functions ----------------------------*/
    // Clean all memory from all caches (extMemPool hard cleanup)
    FreeBlock *releaseMemInCaches(intptr_t startModifiedCnt, int *lockedBinsThreshold, int numOfLockedBins);
    // Soft heap limit (regular cleanup, then maybe hard cleanup)
    void releaseCachesToLimit();

    /*---------------------------------- Utility ----------------------------------*/
    // TODO: move inside IndexedBins class
    static int sizeToBin(size_t size) {
        if (size >= maxBinned_HugePage)
            return HUGE_BIN;
        else if (size < minBinnedSize)
            return NO_BIN;

        int bin = (size - minBinnedSize)/freeBinsStep;

        MALLOC_ASSERT(bin < HUGE_BIN, "Invalid size.");
        return bin;
    }
    static bool toAlignedBin(FreeBlock *block, size_t size) {
        return isAligned((char*)block + size, slabSize) && size >= slabSize;
    }

public:
    /*--------------------- Init, reset, destroy, verify  -------------------------*/
    void init(ExtMemoryPool *extMemoryPool);
    bool destroy();

    void verify();
    void reset();
    bool clean(); // clean on caches cleanup

    /*------------------------- Slab block request --------------------------------*/
    BlockI *getSlabBlock(int num);
    void putSlabBlock(BlockI *block);

    /*-------------------------- Large object request -----------------------------*/
    LargeMemoryBlock *getLargeBlock(size_t size);
    // TODO: make consistent with getLargeBlock
    void returnLargeObject(LargeMemoryBlock *lmb);

    /*-------------------------- Backreference memory request ----------------------*/
    void *getBackRefSpace(size_t size, bool *rawMemUsed);
    void putBackRefSpace(void *b, size_t size, bool rawMemUsed);

    /*----------------------------- Remap object ----------------------------------*/
    void *remap(void *ptr, size_t oldSize, size_t newSize, size_t alignment);

    /*---------------------------- Validation -------------------------------------*/
    bool inUserPool() const;
    bool ptrCanBeValid(void *ptr) const { return usedAddrRange.inRange(ptr); }

    /*-------------------------- Configuration API --------------------------------*/
    // Soft heap limit
    void setRecommendedMaxSize(size_t softLimit) {
        memSoftLimit = softLimit;
        releaseCachesToLimit();
    }

    /*------------------------------- Info ----------------------------------------*/
    size_t getMaxBinnedSize() const;

    /*-------------------------- Testing, statistics ------------------------------*/
#if __TBB_MALLOC_WHITEBOX_TEST
    size_t getTotalMemSize() const { return totalMemSize.load(std::memory_order_relaxed); }
#endif
#if __TBB_MALLOC_BACKEND_STAT
    void reportStat(FILE *f);
private:
    static size_t binToSize(int bin) {
        MALLOC_ASSERT(bin <= HUGE_BIN, "Invalid bin.");

        return bin*freeBinsStep + minBinnedSize;
    }
#endif
};

#endif // __TBB_backend_H
