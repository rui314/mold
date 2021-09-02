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

#ifndef __TBB_large_objects_H
#define __TBB_large_objects_H

//! The list of possible Cache Bin Aggregator operations.
/*  Declared here to avoid Solaris Studio* 12.2 "multiple definitions" error */
enum CacheBinOperationType {
    CBOP_INVALID = 0,
    CBOP_GET,
    CBOP_PUT_LIST,
    CBOP_CLEAN_TO_THRESHOLD,
    CBOP_CLEAN_ALL,
    CBOP_UPDATE_USED_SIZE
};

// The Cache Bin Aggregator operation status list.
// CBST_NOWAIT can be specified for non-blocking operations.
enum CacheBinOperationStatus {
    CBST_WAIT = 0,
    CBST_NOWAIT,
    CBST_DONE
};

/*
 * Bins that grow with arithmetic step
 */
template<size_t MIN_SIZE, size_t MAX_SIZE>
struct LargeBinStructureProps {
public:
    static const size_t   MinSize = MIN_SIZE, MaxSize = MAX_SIZE;
    static const size_t   CacheStep = 8 * 1024;
    static const unsigned NumBins = (MaxSize - MinSize) / CacheStep;

    static size_t alignToBin(size_t size) {
        return alignUp(size, CacheStep);
    }

    static int sizeToIdx(size_t size) {
        MALLOC_ASSERT(MinSize <= size && size < MaxSize, ASSERT_TEXT);
        MALLOC_ASSERT(size % CacheStep == 0, ASSERT_TEXT);
        return (size - MinSize) / CacheStep;
    }
};

/*
 * Bins that grow with special geometric progression.
 */
template<size_t MIN_SIZE, size_t MAX_SIZE>
struct HugeBinStructureProps {

private:
    // Sizes grow with the following formula: Size = MinSize * (2 ^ (Index / StepFactor))
    // There are StepFactor bins (8 be default) between each power of 2 bin
    static const int MaxSizeExp    = Log2<MAX_SIZE>::value;
    static const int MinSizeExp    = Log2<MIN_SIZE>::value;
    static const int StepFactor    = 8;
    static const int StepFactorExp = Log2<StepFactor>::value;

public:
    static const size_t   MinSize = MIN_SIZE, MaxSize = MAX_SIZE;
    static const unsigned NumBins = (MaxSizeExp - MinSizeExp) * StepFactor;

    static size_t alignToBin(size_t size) {
        size_t minorStepExp = BitScanRev(size) - StepFactorExp;
        return alignUp(size, 1ULL << minorStepExp);
    }

    // Sizes between the power of 2 values are aproximated to StepFactor.
    static int sizeToIdx(size_t size) {
        MALLOC_ASSERT(MinSize <= size && size <= MaxSize, ASSERT_TEXT);
        int sizeExp = (int)BitScanRev(size); // same as __TBB_Log2
        size_t majorStepSize = 1ULL << sizeExp;
        int minorStepExp = sizeExp - StepFactorExp;
        int minorIdx = (size - majorStepSize) >> minorStepExp;
        MALLOC_ASSERT(size == majorStepSize + ((size_t)minorIdx << minorStepExp),
            "Size is not aligned on the bin");
        return StepFactor * (sizeExp - MinSizeExp) + minorIdx;
    }
};

/*
 * Cache properties accessor
 *
 * TooLargeFactor -- when cache size treated "too large" in comparison to user data size
 * OnMissFactor -- If cache miss occurred and cache was cleaned,
 *                 set ageThreshold to OnMissFactor * the difference
 *                 between current time and last time cache was cleaned.
 * LongWaitFactor -- to detect rarely-used bins and forget about their usage history
 */
template<typename StructureProps, int TOO_LARGE, int ON_MISS, int LONG_WAIT>
struct LargeObjectCacheProps : public StructureProps {
    static const int TooLargeFactor = TOO_LARGE, OnMissFactor = ON_MISS, LongWaitFactor = LONG_WAIT;
};

template<typename Props>
class LargeObjectCacheImpl {
private:

    // Current sizes of used and cached objects. It's calculated while we are
    // traversing bins, and used for isLOCTooLarge() check at the same time.
    class BinsSummary {
        size_t usedSz;
        size_t cachedSz;
    public:
        BinsSummary() : usedSz(0), cachedSz(0) {}
        // "too large" criteria
        bool isLOCTooLarge() const { return cachedSz > Props::TooLargeFactor * usedSz; }
        void update(size_t usedSize, size_t cachedSize) {
            usedSz += usedSize;
            cachedSz += cachedSize;
        }
        void reset() { usedSz = cachedSz = 0; }
    };

public:
    // The number of bins to cache large/huge objects.
    static const uint32_t numBins = Props::NumBins;

    typedef BitMaskMax<numBins> BinBitMask;

    // 2-linked list of same-size cached blocks ordered by age (oldest on top)
    // TODO: are we really want the list to be 2-linked? This allows us
    // reduce memory consumption and do less operations under lock.
    // TODO: try to switch to 32-bit logical time to save space in CacheBin
    // and move bins to different cache lines.
    class CacheBin {
    private:
        LargeMemoryBlock* first;
        std::atomic<LargeMemoryBlock*> last;
        /* age of an oldest block in the list; equal to last->age, if last defined,
            used for quick checking it without acquiring the lock. */
        std::atomic<uintptr_t> oldest;
        /* currAge when something was excluded out of list because of the age,
         not because of cache hit */
        uintptr_t         lastCleanedAge;
        /* Current threshold value for the blocks of a particular size.
         Set on cache miss. */
        std::atomic<intptr_t> ageThreshold;

        /* total size of all objects corresponding to the bin and allocated by user */
        std::atomic<size_t> usedSize;
        /* total size of all objects cached in the bin */
        std::atomic<size_t> cachedSize;
        /* mean time of presence of block in the bin before successful reuse */
        std::atomic<intptr_t> meanHitRange;
        /* time of last get called for the bin */
        uintptr_t         lastGet;

        typename MallocAggregator<CacheBinOperation>::type aggregator;

        void ExecuteOperation(CacheBinOperation *op, ExtMemoryPool *extMemPool, BinBitMask *bitMask, int idx, bool longLifeTime = true);

        /* should be placed in zero-initialized memory, ctor not needed. */
        CacheBin();

    public:
        void init() {
            memset(this, 0, sizeof(CacheBin));
        }

        /* ---------- Cache accessors ---------- */
        void putList(ExtMemoryPool *extMemPool, LargeMemoryBlock *head, BinBitMask *bitMask, int idx);
        LargeMemoryBlock *get(ExtMemoryPool *extMemPool, size_t size, BinBitMask *bitMask, int idx);

        /* ---------- Cleanup functions -------- */
        bool cleanToThreshold(ExtMemoryPool *extMemPool, BinBitMask *bitMask, uintptr_t currTime, int idx);
        bool releaseAllToBackend(ExtMemoryPool *extMemPool, BinBitMask *bitMask, int idx);
        /* ------------------------------------- */

        void updateUsedSize(ExtMemoryPool *extMemPool, size_t size, BinBitMask *bitMask, int idx);
        void decreaseThreshold() {
            intptr_t threshold = ageThreshold.load(std::memory_order_relaxed);
            if (threshold)
                ageThreshold.store((threshold + meanHitRange.load(std::memory_order_relaxed)) / 2, std::memory_order_relaxed);
        }
        void updateBinsSummary(BinsSummary *binsSummary) const {
            binsSummary->update(usedSize.load(std::memory_order_relaxed), cachedSize.load(std::memory_order_relaxed));
        }
        size_t getSize() const { return cachedSize.load(std::memory_order_relaxed); }
        size_t getUsedSize() const { return usedSize.load(std::memory_order_relaxed); }
        size_t reportStat(int num, FILE *f);

        /* --------- Unsafe methods used with the aggregator ------- */
        void forgetOutdatedState(uintptr_t currTime);
        LargeMemoryBlock *putList(LargeMemoryBlock *head, LargeMemoryBlock *tail, BinBitMask *bitMask,
                int idx, int num, size_t hugeObjectThreshold);
        LargeMemoryBlock *get();
        LargeMemoryBlock *cleanToThreshold(uintptr_t currTime, BinBitMask *bitMask, int idx);
        LargeMemoryBlock *cleanAll(BinBitMask *bitMask, int idx);
        void updateUsedSize(size_t size, BinBitMask *bitMask, int idx) {
            if (!usedSize.load(std::memory_order_relaxed)) bitMask->set(idx, true);
            usedSize.store(usedSize.load(std::memory_order_relaxed) + size, std::memory_order_relaxed);
            if (!usedSize.load(std::memory_order_relaxed) && !first) bitMask->set(idx, false);
        }
        void updateMeanHitRange( intptr_t hitRange ) {
            hitRange = hitRange >= 0 ? hitRange : 0;
            intptr_t mean = meanHitRange.load(std::memory_order_relaxed);
            mean = mean ? (mean + hitRange) / 2 : hitRange;
            meanHitRange.store(mean, std::memory_order_relaxed);
        }
        void updateAgeThreshold( uintptr_t currTime ) {
            if (lastCleanedAge)
                ageThreshold.store(Props::OnMissFactor * (currTime - lastCleanedAge), std::memory_order_relaxed);
        }
        void updateCachedSize(size_t size) {
            cachedSize.store(cachedSize.load(std::memory_order_relaxed) + size, std::memory_order_relaxed);
        }
        void setLastGet( uintptr_t newLastGet ) {
            lastGet = newLastGet;
        }
        /* -------------------------------------------------------- */
    };

    // Huge bins index for fast regular cleanup searching in case of
    // the "huge size threshold" setting defined
    intptr_t     hugeSizeThresholdIdx;

private:
    // How many times LOC was "too large"
    std::atomic<intptr_t> tooLargeLOC;
    // for fast finding of used bins and bins with non-zero usedSize;
    // indexed from the end, as we need largest 1st
    BinBitMask   bitMask;
    // bins with lists of recently freed large blocks cached for re-use
    CacheBin bin[numBins];

public:
    /* ------------ CacheBin structure dependent stuff ------------ */
    static size_t alignToBin(size_t size) {
        return Props::alignToBin(size);
    }
    static int sizeToIdx(size_t size) {
        return Props::sizeToIdx(size);
    }

    /* --------- Main cache functions (put, get object) ------------ */
    void putList(ExtMemoryPool *extMemPool, LargeMemoryBlock *largeBlock);
    LargeMemoryBlock *get(ExtMemoryPool *extMemPool, size_t size);

    /* ------------------------ Cleanup ---------------------------- */
    bool regularCleanup(ExtMemoryPool *extMemPool, uintptr_t currAge, bool doThreshDecr);
    bool cleanAll(ExtMemoryPool *extMemPool);

    /* -------------------------- Other ---------------------------- */
    void updateCacheState(ExtMemoryPool *extMemPool, DecreaseOrIncrease op, size_t size);

    void reset();
    void reportStat(FILE *f);
#if __TBB_MALLOC_WHITEBOX_TEST
    size_t getLOCSize() const;
    size_t getUsedSize() const;
#endif
};

class LargeObjectCache {
private:
    // Large bins [minLargeSize, maxLargeSize)
    // Huge bins [maxLargeSize, maxHugeSize)
    static const size_t minLargeSize = 8 * 1024,
                        maxLargeSize = 8 * 1024 * 1024,
                        // Cache memory up to 1TB (or 2GB for 32-bit arch), but sieve objects from the special threshold
                        maxHugeSize = tbb::detail::select_size_t_constant<2147483648U, 1099511627776ULL>::value;

public:
    // Upper bound threshold for caching size. After that size all objects sieve through cache
    // By default - 64MB, previous value was 129MB (needed by some Intel(R) Math Kernel Library (Intel(R) MKL) benchmarks)
    static const size_t defaultMaxHugeSize = 64UL * 1024UL * 1024UL;
    // After that size large object interpreted as huge and does not participate in regular cleanup.
    // Can be changed during the program execution.
    size_t hugeSizeThreshold;

private:
    // Large objects cache properties
    typedef LargeBinStructureProps<minLargeSize, maxLargeSize> LargeBSProps;
    typedef LargeObjectCacheProps<LargeBSProps, 2, 2, 16> LargeCacheTypeProps;

    // Huge objects cache properties
    typedef HugeBinStructureProps<maxLargeSize, maxHugeSize> HugeBSProps;
    typedef LargeObjectCacheProps<HugeBSProps, 1, 1, 4> HugeCacheTypeProps;

    // Cache implementation type with properties
    typedef LargeObjectCacheImpl< LargeCacheTypeProps > LargeCacheType;
    typedef LargeObjectCacheImpl< HugeCacheTypeProps > HugeCacheType;

    // Beginning of largeCache is more actively used and smaller than hugeCache,
    // so put hugeCache first to prevent false sharing
    // with LargeObjectCache's predecessor
    HugeCacheType hugeCache;
    LargeCacheType largeCache;

    /* logical time, incremented on each put/get operation
       To prevent starvation between pools, keep separately for each pool.
       Overflow is OK, as we only want difference between
       its current value and some recent.

       Both malloc and free should increment logical time, as in
       a different case multiple cached blocks would have same age,
       and accuracy of predictors suffers.
    */
    std::atomic<uintptr_t> cacheCurrTime;

    // Memory pool that owns this LargeObjectCache.
    // strict 1:1 relation, never changed
    ExtMemoryPool *extMemPool;

    // Returns artificial bin index,
    // it's used only during sorting and never saved
    static int sizeToIdx(size_t size);

    // Our friends
    friend class Backend;

public:
    void init(ExtMemoryPool *memPool);

    // Item accessors
    void put(LargeMemoryBlock *largeBlock);
    void putList(LargeMemoryBlock *head);
    LargeMemoryBlock *get(size_t size);

    void updateCacheState(DecreaseOrIncrease op, size_t size);
    bool isCleanupNeededOnRange(uintptr_t range, uintptr_t currTime);

    // Cleanup operations
    bool doCleanup(uintptr_t currTime, bool doThreshDecr);
    bool decreasingCleanup();
    bool regularCleanup();
    bool cleanAll();
    void reset();

    void reportStat(FILE *f);
#if __TBB_MALLOC_WHITEBOX_TEST
    size_t getLOCSize() const;
    size_t getUsedSize() const;
#endif

    // Cache deals with exact-fit sizes, so need to align each size
    // to the specific bin when put object to cache
    static size_t alignToBin(size_t size);

    void setHugeSizeThreshold(size_t value);

    // Check if we should cache or sieve this size
    bool sizeInCacheRange(size_t size);

    uintptr_t getCurrTimeRange(uintptr_t range);
    void registerRealloc(size_t oldSize, size_t newSize);
};

#endif // __TBB_large_objects_H

