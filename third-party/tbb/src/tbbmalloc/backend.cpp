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

#include <string.h>   /* for memset */
#include <errno.h>
#include "tbbmalloc_internal.h"

namespace rml {
namespace internal {

/*********** Code to acquire memory from the OS or other executive ****************/

/*
  syscall/malloc can set non-zero errno in case of failure,
  but later allocator might be able to find memory to fulfill the request.
  And we do not want changing of errno by successful scalable_malloc call.
  To support this, restore old errno in (get|free)RawMemory, and set errno
  in frontend just before returning to user code.
  Please note: every syscall/libc call used inside scalable_malloc that
  sets errno must be protected this way, not just memory allocation per se.
*/

#if USE_DEFAULT_MEMORY_MAPPING
#include "MapMemory.h"
#else
/* assume MapMemory and UnmapMemory are customized */
#endif

void* getRawMemory (size_t size, PageType pageType) {
    return MapMemory(size, pageType);
}

int freeRawMemory (void *object, size_t size) {
    return UnmapMemory(object, size);
}

#if CHECK_ALLOCATION_RANGE

void Backend::UsedAddressRange::registerAlloc(uintptr_t left, uintptr_t right)
{
    MallocMutex::scoped_lock lock(mutex);
    if (left < leftBound.load(std::memory_order_relaxed))
        leftBound.store(left, std::memory_order_relaxed);
    if (right > rightBound.load(std::memory_order_relaxed))
        rightBound.store(right, std::memory_order_relaxed);
    MALLOC_ASSERT(leftBound.load(std::memory_order_relaxed), ASSERT_TEXT);
    MALLOC_ASSERT(leftBound.load(std::memory_order_relaxed) < rightBound.load(std::memory_order_relaxed), ASSERT_TEXT);
    MALLOC_ASSERT(leftBound.load(std::memory_order_relaxed) <= left && right <= rightBound.load(std::memory_order_relaxed), ASSERT_TEXT);
}

void Backend::UsedAddressRange::registerFree(uintptr_t left, uintptr_t right)
{
    MallocMutex::scoped_lock lock(mutex);
    if (leftBound.load(std::memory_order_relaxed) == left) {
        if (rightBound.load(std::memory_order_relaxed) == right) {
            leftBound.store(ADDRESS_UPPER_BOUND, std::memory_order_relaxed);
            rightBound.store(0, std::memory_order_relaxed);
        } else
            leftBound.store(right, std::memory_order_relaxed);
    } else if (rightBound.load(std::memory_order_relaxed) == right)
        rightBound.store(left, std::memory_order_relaxed);
    MALLOC_ASSERT((!rightBound.load(std::memory_order_relaxed) && leftBound.load(std::memory_order_relaxed) == ADDRESS_UPPER_BOUND)
                  || leftBound.load(std::memory_order_relaxed) < rightBound.load(std::memory_order_relaxed), ASSERT_TEXT);
}
#endif // CHECK_ALLOCATION_RANGE

// Initialized in frontend inside defaultMemPool
extern HugePagesStatus hugePages;

void *Backend::allocRawMem(size_t &size)
{
    void *res = nullptr;
    size_t allocSize = 0;

    if (extMemPool->userPool()) {
        if (extMemPool->fixedPool && bootsrapMemDone == bootsrapMemStatus.load(std::memory_order_acquire))
            return nullptr;
        MALLOC_ASSERT(bootsrapMemStatus != bootsrapMemNotDone,
                      "Backend::allocRawMem() called prematurely?");
        // TODO: support for raw mem not aligned at sizeof(uintptr_t)
        // memory from fixed pool is asked once and only once
        allocSize = alignUpGeneric(size, extMemPool->granularity);
        res = (*extMemPool->rawAlloc)(extMemPool->poolId, allocSize);
    } else {
        // Align allocation on page size
        size_t pageSize = hugePages.isEnabled ? hugePages.getGranularity() : extMemPool->granularity;
        MALLOC_ASSERT(pageSize, "Page size cannot be zero.");
        allocSize = alignUpGeneric(size, pageSize);

        // If user requested huge pages and they are available, try to use preallocated ones firstly.
        // If there are none, lets check transparent huge pages support and use them instead.
        if (hugePages.isEnabled) {
            if (hugePages.isHPAvailable) {
                res = getRawMemory(allocSize, PREALLOCATED_HUGE_PAGE);
            }
            if (!res && hugePages.isTHPAvailable) {
                res = getRawMemory(allocSize, TRANSPARENT_HUGE_PAGE);
            }
        }

        if (!res) {
            res = getRawMemory(allocSize, REGULAR);
        }
    }

    if (res) {
        MALLOC_ASSERT(allocSize > 0, "Invalid size of an allocated region.");
        size = allocSize;
        if (!extMemPool->userPool())
            usedAddrRange.registerAlloc((uintptr_t)res, (uintptr_t)res+size);
#if MALLOC_DEBUG
        volatile size_t curTotalSize = totalMemSize; // to read global value once
        MALLOC_ASSERT(curTotalSize+size > curTotalSize, "Overflow allocation size.");
#endif
        totalMemSize.fetch_add(size);
    }

    return res;
}

bool Backend::freeRawMem(void *object, size_t size)
{
    bool fail;
#if MALLOC_DEBUG
    volatile size_t curTotalSize = totalMemSize; // to read global value once
    MALLOC_ASSERT(curTotalSize-size < curTotalSize, "Negative allocation size.");
#endif
    totalMemSize.fetch_sub(size);
    if (extMemPool->userPool()) {
        MALLOC_ASSERT(!extMemPool->fixedPool, "No free for fixed-size pools.");
        fail = (*extMemPool->rawFree)(extMemPool->poolId, object, size);
    } else {
        usedAddrRange.registerFree((uintptr_t)object, (uintptr_t)object + size);
        fail = freeRawMemory(object, size);
    }
    // TODO: use result in all freeRawMem() callers
    return !fail;
}

/********* End memory acquisition code ********************************/

// Protected object size. After successful locking returns size of locked block,
// and releasing requires setting block size.
class GuardedSize : tbb::detail::no_copy {
    std::atomic<uintptr_t> value;
public:
    enum State {
        LOCKED,
        COAL_BLOCK,        // block is coalescing now
        MAX_LOCKED_VAL = COAL_BLOCK,
        LAST_REGION_BLOCK, // used to mark last block in region
        // values after this are "normal" block sizes
        MAX_SPEC_VAL = LAST_REGION_BLOCK
    };

    void initLocked() { value.store(LOCKED, std::memory_order_release); } // TBB_REVAMP_TODO: was relaxed
    void makeCoalscing() {
        MALLOC_ASSERT(value.load(std::memory_order_relaxed) == LOCKED, ASSERT_TEXT);
        value.store(COAL_BLOCK, std::memory_order_release); // TBB_REVAMP_TODO: was relaxed
    }
    size_t tryLock(State state) {
        MALLOC_ASSERT(state <= MAX_LOCKED_VAL, ASSERT_TEXT);
        size_t sz = value.load(std::memory_order_acquire);
        for (;;) {
            if (sz <= MAX_LOCKED_VAL) {
                break;
            }
            if (value.compare_exchange_strong(sz, state)) {
                break;
            }
        }
        return sz;
    }
    void unlock(size_t size) {
        MALLOC_ASSERT(value.load(std::memory_order_relaxed) <= MAX_LOCKED_VAL, "The lock is not locked");
        MALLOC_ASSERT(size > MAX_LOCKED_VAL, ASSERT_TEXT);
        value.store(size, std::memory_order_release);
    }
    bool isLastRegionBlock() const { return value.load(std::memory_order_relaxed) == LAST_REGION_BLOCK; }
    friend void Backend::IndexedBins::verify();
};

struct MemRegion {
    MemRegion *next,      // keep all regions in any pool to release all them on
              *prev;      // pool destroying, 2-linked list to release individual
                          // regions.
    size_t     allocSz,   // got from pool callback
               blockSz;   // initial and maximal inner block size
    MemRegionType type;
};

// this data must be unmodified while block is in use, so separate it
class BlockMutexes {
protected:
    GuardedSize myL,   // lock for me
                leftL; // lock for left neighbor
};

class FreeBlock : BlockMutexes {
public:
    static const size_t minBlockSize;
    friend void Backend::IndexedBins::verify();

    FreeBlock    *prev,       // in 2-linked list related to bin
                 *next,
                 *nextToFree; // used to form a queue during coalescing
    // valid only when block is in processing, i.e. one is not free and not
    size_t        sizeTmp;    // used outside of backend
    int           myBin;      // bin that is owner of the block
    bool          slabAligned;
    bool          blockInBin; // this block in myBin already

    FreeBlock *rightNeig(size_t sz) const {
        MALLOC_ASSERT(sz, ASSERT_TEXT);
        return (FreeBlock*)((uintptr_t)this+sz);
    }
    FreeBlock *leftNeig(size_t sz) const {
        MALLOC_ASSERT(sz, ASSERT_TEXT);
        return (FreeBlock*)((uintptr_t)this - sz);
    }

    void initHeader() { myL.initLocked(); leftL.initLocked(); }
    void setMeFree(size_t size) { myL.unlock(size); }
    size_t trySetMeUsed(GuardedSize::State s) { return myL.tryLock(s); }
    bool isLastRegionBlock() const { return myL.isLastRegionBlock(); }

    void setLeftFree(size_t sz) { leftL.unlock(sz); }
    size_t trySetLeftUsed(GuardedSize::State s) { return leftL.tryLock(s); }

    size_t tryLockBlock() {
        size_t rSz, sz = trySetMeUsed(GuardedSize::LOCKED);

        if (sz <= GuardedSize::MAX_LOCKED_VAL)
            return false;
        rSz = rightNeig(sz)->trySetLeftUsed(GuardedSize::LOCKED);
        if (rSz <= GuardedSize::MAX_LOCKED_VAL) {
            setMeFree(sz);
            return false;
        }
        MALLOC_ASSERT(rSz == sz, ASSERT_TEXT);
        return sz;
    }
    void markCoalescing(size_t blockSz) {
        myL.makeCoalscing();
        rightNeig(blockSz)->leftL.makeCoalscing();
        sizeTmp = blockSz;
        nextToFree = nullptr;
    }
    void markUsed() {
        myL.initLocked();
        rightNeig(sizeTmp)->leftL.initLocked();
        nextToFree = nullptr;
    }
    static void markBlocks(FreeBlock *fBlock, int num, size_t size) {
        for (int i=1; i<num; i++) {
            fBlock = (FreeBlock*)((uintptr_t)fBlock + size);
            fBlock->initHeader();
        }
    }
};

// Last block in any region. Its "size" field is GuardedSize::LAST_REGION_BLOCK,
// This kind of blocks used to find region header
// and have a possibility to return region back to OS
struct LastFreeBlock : public FreeBlock {
    MemRegion *memRegion;
};

const size_t FreeBlock::minBlockSize = sizeof(FreeBlock);

inline bool BackendSync::waitTillBlockReleased(intptr_t startModifiedCnt)
{
    AtomicBackoff backoff;
#if __TBB_MALLOC_BACKEND_STAT
    class ITT_Guard {
        void *ptr;
    public:
        ITT_Guard(void *p) : ptr(p) {
            MALLOC_ITT_SYNC_PREPARE(ptr);
        }
        ~ITT_Guard() {
            MALLOC_ITT_SYNC_ACQUIRED(ptr);
        }
    };
    ITT_Guard ittGuard(&inFlyBlocks);
#endif
    for (intptr_t myBinsInFlyBlocks = inFlyBlocks.load(std::memory_order_acquire),
             myCoalescQInFlyBlocks = backend->blocksInCoalescing(); ; backoff.pause()) {
        MALLOC_ASSERT(myBinsInFlyBlocks>=0 && myCoalescQInFlyBlocks>=0, nullptr);
        intptr_t currBinsInFlyBlocks = inFlyBlocks.load(std::memory_order_acquire),
            currCoalescQInFlyBlocks = backend->blocksInCoalescing();
        WhiteboxTestingYield();
        // Stop waiting iff:

        // 1) blocks were removed from processing, not added
        if (myBinsInFlyBlocks > currBinsInFlyBlocks
        // 2) released during delayed coalescing queue
            || myCoalescQInFlyBlocks > currCoalescQInFlyBlocks)
            break;
        // 3) if there are blocks in coalescing, and no progress in its processing,
        // try to scan coalescing queue and stop waiting, if changes were made
        // (if there are no changes and in-fly blocks exist, we continue
        //  waiting to not increase load on coalescQ)
        if (currCoalescQInFlyBlocks > 0 && backend->scanCoalescQ(/*forceCoalescQDrop=*/false))
            break;
        // 4) when there are no blocks
        if (!currBinsInFlyBlocks && !currCoalescQInFlyBlocks)
            // re-scan make sense only if bins were modified since scanned
            return startModifiedCnt != getNumOfMods();
        myBinsInFlyBlocks = currBinsInFlyBlocks;
        myCoalescQInFlyBlocks = currCoalescQInFlyBlocks;
    }
    return true;
}

void CoalRequestQ::putBlock(FreeBlock *fBlock)
{
    MALLOC_ASSERT(fBlock->sizeTmp >= FreeBlock::minBlockSize, ASSERT_TEXT);
    fBlock->markUsed();
    // the block is in the queue, do not forget that it's here
    inFlyBlocks++;

    FreeBlock *myBlToFree = blocksToFree.load(std::memory_order_acquire);
    for (;;) {
        fBlock->nextToFree = myBlToFree;
        if (blocksToFree.compare_exchange_strong(myBlToFree, fBlock)) {
            return;
        }
    }
}

FreeBlock *CoalRequestQ::getAll()
{
    for (;;) {
        FreeBlock *myBlToFree = blocksToFree.load(std::memory_order_acquire);

        if (!myBlToFree) {
            return nullptr;
        } else {
            if (blocksToFree.compare_exchange_strong(myBlToFree, nullptr)) {
                return myBlToFree;
            } else {
                continue;
            }
        }
    }
}

inline void CoalRequestQ::blockWasProcessed()
{
    bkndSync->binsModified();
    int prev = inFlyBlocks.fetch_sub(1);
    MALLOC_ASSERT(prev > 0, ASSERT_TEXT);
}

// Try to get a block from a bin.
// If the remaining free space would stay in the same bin,
//     split the block without removing it.
// If the free space should go to other bin(s), remove the block.
// alignedBin is true, if all blocks in the bin have slab-aligned right side.
FreeBlock *Backend::IndexedBins::getFromBin(int binIdx, BackendSync *sync, size_t size,
        bool needAlignedRes, bool alignedBin,  bool wait, int *binLocked)
{
    Bin *b = &freeBins[binIdx];
try_next:
    FreeBlock *fBlock = nullptr;
    if (!b->empty()) {
        bool locked;
        MallocMutex::scoped_lock scopedLock(b->tLock, wait, &locked);

        if (!locked) {
            if (binLocked) (*binLocked)++;
            return nullptr;
        }

        for (FreeBlock *curr = b->head.load(std::memory_order_relaxed); curr; curr = curr->next) {
            size_t szBlock = curr->tryLockBlock();
            if (!szBlock) {
                // block is locked, re-do bin lock, as there is no place to spin
                // while block coalescing
                goto try_next;
            }

            // GENERAL CASE
            if (alignedBin || !needAlignedRes) {
                size_t splitSz = szBlock - size;
                // If we got a block as split result, it must have a room for control structures.
                if (szBlock >= size && (splitSz >= FreeBlock::minBlockSize || !splitSz))
                    fBlock = curr;
            } else {
                // SPECIAL CASE, to get aligned block from unaligned bin we have to cut the middle of a block
                // and return remaining left and right part. Possible only in fixed pool scenario, assert for this
                // is set inside splitBlock() function.

                void *newB = alignUp(curr, slabSize);
                uintptr_t rightNew = (uintptr_t)newB + size;
                uintptr_t rightCurr = (uintptr_t)curr + szBlock;
                // Check if the block size is sufficient,
                // and also left and right split results are either big enough or non-existent
                if (rightNew <= rightCurr
                        && (newB == curr || ((uintptr_t)newB - (uintptr_t)curr) >= FreeBlock::minBlockSize)
                        && (rightNew == rightCurr || (rightCurr - rightNew) >= FreeBlock::minBlockSize))
                    fBlock = curr;
            }

            if (fBlock) {
                // consume must be called before result of removing from a bin is visible externally.
                sync->blockConsumed();
                // TODO: think about cases when block stays in the same bin
                b->removeBlock(fBlock);
                if (freeBins[binIdx].empty())
                    bitMask.set(binIdx, false);
                fBlock->sizeTmp = szBlock;
                break;
            } else { // block size is not valid, search for next block in the bin
                curr->setMeFree(szBlock);
                curr->rightNeig(szBlock)->setLeftFree(szBlock);
            }
        }
    }
    return fBlock;
}

bool Backend::IndexedBins::tryReleaseRegions(int binIdx, Backend *backend)
{
    Bin *b = &freeBins[binIdx];
    FreeBlock *fBlockList = nullptr;

    // got all blocks from the bin and re-do coalesce on them
    // to release single-block regions
try_next:
    if (!b->empty()) {
        MallocMutex::scoped_lock binLock(b->tLock);
        for (FreeBlock *curr = b->head.load(std::memory_order_relaxed); curr; ) {
            size_t szBlock = curr->tryLockBlock();
            if (!szBlock)
                goto try_next;

            FreeBlock *next = curr->next;

            b->removeBlock(curr);
            curr->sizeTmp = szBlock;
            curr->nextToFree = fBlockList;
            fBlockList = curr;
            curr = next;
        }
    }
    return backend->coalescAndPutList(fBlockList, /*forceCoalescQDrop=*/true,
                                      /*reportBlocksProcessed=*/false);
}

void Backend::Bin::removeBlock(FreeBlock *fBlock)
{
    MALLOC_ASSERT(fBlock->next||fBlock->prev||fBlock== head.load(std::memory_order_relaxed),
                  "Detected that a block is not in the bin.");
    if (head.load(std::memory_order_relaxed) == fBlock)
        head.store(fBlock->next, std::memory_order_relaxed);
    if (tail == fBlock)
        tail = fBlock->prev;
    if (fBlock->prev)
        fBlock->prev->next = fBlock->next;
    if (fBlock->next)
        fBlock->next->prev = fBlock->prev;
}

void Backend::IndexedBins::addBlock(int binIdx, FreeBlock *fBlock, size_t /* blockSz */, bool addToTail)
{
    Bin *b = &freeBins[binIdx];
    fBlock->myBin = binIdx;
    fBlock->next = fBlock->prev = nullptr;
    {
        MallocMutex::scoped_lock scopedLock(b->tLock);
        if (addToTail) {
            fBlock->prev = b->tail;
            b->tail = fBlock;
            if (fBlock->prev)
                fBlock->prev->next = fBlock;
            if (!b->head.load(std::memory_order_relaxed))
                b->head.store(fBlock, std::memory_order_relaxed);
        } else {
            fBlock->next = b->head.load(std::memory_order_relaxed);
            b->head.store(fBlock, std::memory_order_relaxed);
            if (fBlock->next)
                fBlock->next->prev = fBlock;
            if (!b->tail)
                b->tail = fBlock;
        }
    }
    bitMask.set(binIdx, true);
}

bool Backend::IndexedBins::tryAddBlock(int binIdx, FreeBlock *fBlock, bool addToTail)
{
    bool locked;
    Bin *b = &freeBins[binIdx];
    fBlock->myBin = binIdx;
    if (addToTail) {
        fBlock->next = nullptr;
        {
            MallocMutex::scoped_lock scopedLock(b->tLock, /*wait=*/false, &locked);
            if (!locked)
                return false;
            fBlock->prev = b->tail;
            b->tail = fBlock;
            if (fBlock->prev)
                fBlock->prev->next = fBlock;
            if (!b->head.load(std::memory_order_relaxed))
                b->head.store(fBlock, std::memory_order_relaxed);
        }
    } else {
        fBlock->prev = nullptr;
        {
            MallocMutex::scoped_lock scopedLock(b->tLock, /*wait=*/false, &locked);
            if (!locked)
                return false;
            fBlock->next = b->head.load(std::memory_order_relaxed);
            b->head.store(fBlock, std::memory_order_relaxed);
            if (fBlock->next)
                fBlock->next->prev = fBlock;
            if (!b->tail)
                b->tail = fBlock;
        }
    }
    bitMask.set(binIdx, true);
    return true;
}

void Backend::IndexedBins::reset()
{
    for (unsigned i=0; i<Backend::freeBinsNum; i++)
        freeBins[i].reset();
    bitMask.reset();
}

void Backend::IndexedBins::lockRemoveBlock(int binIdx, FreeBlock *fBlock)
{
    MallocMutex::scoped_lock scopedLock(freeBins[binIdx].tLock);
    freeBins[binIdx].removeBlock(fBlock);
    if (freeBins[binIdx].empty())
        bitMask.set(binIdx, false);
}

bool ExtMemoryPool::regionsAreReleaseable() const
{
    return !keepAllMemory && !delayRegsReleasing;
}

FreeBlock *Backend::splitBlock(FreeBlock *fBlock, int num, size_t size, bool blockIsAligned, bool needAlignedBlock)
{
    const size_t totalSize = num * size;

    // SPECIAL CASE, for unaligned block we have to cut the middle of a block
    // and return remaining left and right part. Possible only in a fixed pool scenario.
    if (needAlignedBlock && !blockIsAligned) {
        MALLOC_ASSERT(extMemPool->fixedPool,
                "Aligned block request from unaligned bin possible only in fixed pool scenario.");

        // Space to use is in the middle
        FreeBlock *newBlock = alignUp(fBlock, slabSize);
        FreeBlock *rightPart = (FreeBlock*)((uintptr_t)newBlock + totalSize);
        uintptr_t fBlockEnd = (uintptr_t)fBlock + fBlock->sizeTmp;

        // Return free right part
        if ((uintptr_t)rightPart != fBlockEnd) {
            rightPart->initHeader();  // to prevent coalescing rightPart with fBlock
            size_t rightSize = fBlockEnd - (uintptr_t)rightPart;
            coalescAndPut(rightPart, rightSize, toAlignedBin(rightPart, rightSize));
        }
        // And free left part
        if (newBlock != fBlock) {
            newBlock->initHeader(); // to prevent coalescing fBlock with newB
            size_t leftSize = (uintptr_t)newBlock - (uintptr_t)fBlock;
            coalescAndPut(fBlock, leftSize, toAlignedBin(fBlock, leftSize));
        }
        fBlock = newBlock;
    } else if (size_t splitSize = fBlock->sizeTmp - totalSize) { // need to split the block
        // GENERAL CASE, cut the left or right part of the block
        FreeBlock *splitBlock = nullptr;
        if (needAlignedBlock) {
            // For slab aligned blocks cut the right side of the block
            // and return it to a requester, original block returns to backend
            splitBlock = fBlock;
            fBlock = (FreeBlock*)((uintptr_t)splitBlock + splitSize);
            fBlock->initHeader();
        } else {
            // For large object blocks cut original block and put free righ part to backend
            splitBlock = (FreeBlock*)((uintptr_t)fBlock + totalSize);
            splitBlock->initHeader();
        }
        // Mark free block as it`s parent only when the requested type (needAlignedBlock)
        // and returned from Bins/OS block (isAligned) are equal (XOR operation used)
        bool markAligned = (blockIsAligned ^ needAlignedBlock) ? toAlignedBin(splitBlock, splitSize) : blockIsAligned;
        coalescAndPut(splitBlock, splitSize, markAligned);
    }
    MALLOC_ASSERT(!needAlignedBlock || isAligned(fBlock, slabSize), "Expect to get aligned block, if one was requested.");
    FreeBlock::markBlocks(fBlock, num, size);
    return fBlock;
}

size_t Backend::getMaxBinnedSize() const
{
    return hugePages.isEnabled && !inUserPool() ?
        maxBinned_HugePage : maxBinned_SmallPage;
}

inline bool Backend::MaxRequestComparator::operator()(size_t oldMaxReq, size_t requestSize) const
{
    return requestSize > oldMaxReq && requestSize < backend->getMaxBinnedSize();
}

// last chance to get memory
FreeBlock *Backend::releaseMemInCaches(intptr_t startModifiedCnt,
                                    int *lockedBinsThreshold, int numOfLockedBins)
{
    // something released from caches
    if (extMemPool->hardCachesCleanup()
        // ..or can use blocks that are in processing now
        || bkndSync.waitTillBlockReleased(startModifiedCnt))
        return (FreeBlock*)VALID_BLOCK_IN_BIN;
    // OS can't give us more memory, but we have some in locked bins
    if (*lockedBinsThreshold && numOfLockedBins) {
        *lockedBinsThreshold = 0;
        return (FreeBlock*)VALID_BLOCK_IN_BIN;
    }
    return nullptr; // nothing found, give up
}

FreeBlock *Backend::askMemFromOS(size_t blockSize, intptr_t startModifiedCnt,
                                 int *lockedBinsThreshold, int numOfLockedBins,
                                 bool *splittableRet, bool needSlabRegion)
{
    FreeBlock *block;
    // The block sizes can be divided into 3 groups:
    //   1. "quite small": popular object size, we are in bootstarp or something
    //      like; request several regions.
    //   2. "quite large": we want to have several such blocks in the region
    //      but not want several pre-allocated regions.
    //   3. "huge": exact fit, we allocate only one block and do not allow
    //       any other allocations to placed in a region.
    // Dividing the block sizes in these groups we are trying to balance between
    // too small regions (that leads to fragmentation) and too large ones (that
    // leads to excessive address space consumption). If a region is "too
    // large", allocate only one, to prevent fragmentation. It supposedly
    // doesn't hurt performance, because the object requested by user is large.
    // Bounds for the groups are:
    const size_t maxBinned = getMaxBinnedSize();
    const size_t quiteSmall = maxBinned / 8;
    const size_t quiteLarge = maxBinned;

    if (blockSize >= quiteLarge) {
        // Do not interact with other threads via semaphores, as for exact fit
        // we can't share regions with them, memory requesting is individual.
        block = addNewRegion(blockSize, MEMREG_ONE_BLOCK, /*addToBin=*/false);
        if (!block)
            return releaseMemInCaches(startModifiedCnt, lockedBinsThreshold, numOfLockedBins);
        *splittableRet = false;
    } else {
        const size_t regSz_sizeBased = alignUp(4*maxRequestedSize, 1024*1024);
        // Another thread is modifying backend while we can't get the block.
        // Wait while it leaves and re-do the scan
        // before trying other ways to extend the backend.
        if (bkndSync.waitTillBlockReleased(startModifiedCnt)
            // semaphore is protecting adding more more memory from OS
            || memExtendingSema.wait())
            return (FreeBlock*)VALID_BLOCK_IN_BIN;

        if (startModifiedCnt != bkndSync.getNumOfMods()) {
            memExtendingSema.signal();
            return (FreeBlock*)VALID_BLOCK_IN_BIN;
        }

        if (blockSize < quiteSmall) {
            // For this size of blocks, add NUM_OF_REG "advance" regions in bin,
            // and return one as a result.
            // TODO: add to bin first, because other threads can use them right away.
            // This must be done carefully, because blocks in bins can be released
            // in releaseCachesToLimit().
            const unsigned NUM_OF_REG = 3;
            MemRegionType regType = needSlabRegion ? MEMREG_SLAB_BLOCKS : MEMREG_LARGE_BLOCKS;
            block = addNewRegion(regSz_sizeBased, regType, /*addToBin=*/false);
            if (block)
                for (unsigned idx=0; idx<NUM_OF_REG; idx++)
                    if (! addNewRegion(regSz_sizeBased, regType, /*addToBin=*/true))
                        break;
        } else {
            block = addNewRegion(regSz_sizeBased, MEMREG_LARGE_BLOCKS, /*addToBin=*/false);
        }
        memExtendingSema.signal();

        // no regions found, try to clean cache
        if (!block || block == (FreeBlock*)VALID_BLOCK_IN_BIN)
            return releaseMemInCaches(startModifiedCnt, lockedBinsThreshold, numOfLockedBins);
        // Since a region can hold more than one block it can be split.
        *splittableRet = true;
    }
    // after asking memory from OS, release caches if we above the memory limits
    releaseCachesToLimit();

    return block;
}

void Backend::releaseCachesToLimit()
{
    if (!memSoftLimit.load(std::memory_order_relaxed)
            || totalMemSize.load(std::memory_order_relaxed) <= memSoftLimit.load(std::memory_order_relaxed)) {
        return;
    }
    size_t locTotalMemSize, locMemSoftLimit;

    scanCoalescQ(/*forceCoalescQDrop=*/false);
    if (extMemPool->softCachesCleanup() &&
        (locTotalMemSize = totalMemSize.load(std::memory_order_acquire)) <=
        (locMemSoftLimit = memSoftLimit.load(std::memory_order_acquire)))
        return;
    // clean global large-object cache, if this is not enough, clean local caches
    // do this in several tries, because backend fragmentation can prevent
    // region from releasing
    for (int cleanLocal = 0; cleanLocal<2; cleanLocal++)
        while (cleanLocal ?
                 extMemPool->allLocalCaches.cleanup(/*cleanOnlyUnused=*/true) :
                 extMemPool->loc.decreasingCleanup())
            if ((locTotalMemSize = totalMemSize.load(std::memory_order_acquire)) <=
                (locMemSoftLimit = memSoftLimit.load(std::memory_order_acquire)))
                return;
    // last chance to match memSoftLimit
    extMemPool->hardCachesCleanup();
}

int Backend::IndexedBins::getMinNonemptyBin(unsigned startBin) const
{
    int p = bitMask.getMinTrue(startBin);
    return p == -1 ? Backend::freeBinsNum : p;
}

FreeBlock *Backend::IndexedBins::findBlock(int nativeBin, BackendSync *sync, size_t size,
        bool needAlignedBlock, bool alignedBin, int *numOfLockedBins)
{
    for (int i=getMinNonemptyBin(nativeBin); i<freeBinsNum; i=getMinNonemptyBin(i+1))
        if (FreeBlock *block = getFromBin(i, sync, size, needAlignedBlock, alignedBin, /*wait=*/false, numOfLockedBins))
            return block;

    return nullptr;
}

void Backend::requestBootstrapMem()
{
    if (bootsrapMemDone == bootsrapMemStatus.load(std::memory_order_acquire))
        return;
    MallocMutex::scoped_lock lock( bootsrapMemStatusMutex );
    if (bootsrapMemDone == bootsrapMemStatus)
        return;
    MALLOC_ASSERT(bootsrapMemNotDone == bootsrapMemStatus, ASSERT_TEXT);
    bootsrapMemStatus = bootsrapMemInitializing;
    // request some rather big region during bootstrap in advance
    // ok to get nullptr here, as later we re-do a request with more modest size
    addNewRegion(2*1024*1024, MEMREG_SLAB_BLOCKS, /*addToBin=*/true);
    bootsrapMemStatus = bootsrapMemDone;
}

// try to allocate size Byte block in available bins
// needAlignedRes is true if result must be slab-aligned
FreeBlock *Backend::genericGetBlock(int num, size_t size, bool needAlignedBlock)
{
    FreeBlock *block = nullptr;
    const size_t totalReqSize = num*size;
    // no splitting after requesting new region, asks exact size
    const int nativeBin = sizeToBin(totalReqSize);

    requestBootstrapMem();
    // If we found 2 or less locked bins, it's time to ask more memory from OS.
    // But nothing can be asked from fixed pool. And we prefer wait, not ask
    // for more memory, if block is quite large.
    int lockedBinsThreshold = extMemPool->fixedPool || size>=maxBinned_SmallPage? 0 : 2;

    // Find maximal requested size limited by getMaxBinnedSize()
    AtomicUpdate(maxRequestedSize, totalReqSize, MaxRequestComparator(this));
    scanCoalescQ(/*forceCoalescQDrop=*/false);

    bool splittable = true;
    for (;;) {
        const intptr_t startModifiedCnt = bkndSync.getNumOfMods();
        int numOfLockedBins;

        do {
            numOfLockedBins = 0;
            if (needAlignedBlock) {
                block = freeSlabAlignedBins.findBlock(nativeBin, &bkndSync, num*size, needAlignedBlock,
                                                        /*alignedBin=*/true, &numOfLockedBins);
                if (!block && extMemPool->fixedPool)
                    block = freeLargeBlockBins.findBlock(nativeBin, &bkndSync, num*size, needAlignedBlock,
                                                        /*alignedBin=*/false, &numOfLockedBins);
            } else {
                block = freeLargeBlockBins.findBlock(nativeBin, &bkndSync, num*size, needAlignedBlock,
                                                        /*alignedBin=*/false, &numOfLockedBins);
                if (!block && extMemPool->fixedPool)
                    block = freeSlabAlignedBins.findBlock(nativeBin, &bkndSync, num*size, needAlignedBlock,
                                                        /*alignedBin=*/true, &numOfLockedBins);
            }
        } while (!block && numOfLockedBins>lockedBinsThreshold);

        if (block)
            break;

        if (!(scanCoalescQ(/*forceCoalescQDrop=*/true) | extMemPool->softCachesCleanup())) {
            // bins are not updated,
            // only remaining possibility is to ask for more memory
            block = askMemFromOS(totalReqSize, startModifiedCnt, &lockedBinsThreshold,
                        numOfLockedBins, &splittable, needAlignedBlock);
            if (!block)
                return nullptr;
            if (block != (FreeBlock*)VALID_BLOCK_IN_BIN) {
                // size can be increased in askMemFromOS, that's why >=
                MALLOC_ASSERT(block->sizeTmp >= size, ASSERT_TEXT);
                break;
            }
            // valid block somewhere in bins, let's find it
            block = nullptr;
        }
    }
    MALLOC_ASSERT(block, ASSERT_TEXT);
    if (splittable) {
        // At this point we have to be sure that slabAligned attribute describes the right block state
        block = splitBlock(block, num, size, block->slabAligned, needAlignedBlock);
    }
    // matched blockConsumed() from startUseBlock()
    bkndSync.blockReleased();

    return block;
}

LargeMemoryBlock *Backend::getLargeBlock(size_t size)
{
    LargeMemoryBlock *lmb =
        (LargeMemoryBlock*)genericGetBlock(1, size, /*needAlignedRes=*/false);
    if (lmb) {
        lmb->unalignedSize = size;
        if (extMemPool->userPool())
            extMemPool->lmbList.add(lmb);
    }
    return lmb;
}

BlockI *Backend::getSlabBlock(int num) {
    BlockI *b = (BlockI*)genericGetBlock(num, slabSize, /*slabAligned=*/true);
    MALLOC_ASSERT(isAligned(b, slabSize), ASSERT_TEXT);
    return b;
}

void Backend::putSlabBlock(BlockI *block) {
    genericPutBlock((FreeBlock *)block, slabSize, /*slabAligned=*/true);
}

void *Backend::getBackRefSpace(size_t size, bool *rawMemUsed)
{
    // This block is released only at shutdown, so it can prevent
    // a entire region releasing when it's received from the backend,
    // so prefer getRawMemory using.
    if (void *ret = getRawMemory(size, REGULAR)) {
        *rawMemUsed = true;
        return ret;
    }
    void *ret = genericGetBlock(1, size, /*needAlignedRes=*/false);
    if (ret) *rawMemUsed = false;
    return ret;
}

void Backend::putBackRefSpace(void *b, size_t size, bool rawMemUsed)
{
    if (rawMemUsed)
        freeRawMemory(b, size);
    // ignore not raw mem, as it released on region releasing
}

void Backend::removeBlockFromBin(FreeBlock *fBlock)
{
    if (fBlock->myBin != Backend::NO_BIN) {
        if (fBlock->slabAligned)
            freeSlabAlignedBins.lockRemoveBlock(fBlock->myBin, fBlock);
        else
            freeLargeBlockBins.lockRemoveBlock(fBlock->myBin, fBlock);
    }
}

void Backend::genericPutBlock(FreeBlock *fBlock, size_t blockSz, bool slabAligned)
{
    bkndSync.blockConsumed();
    coalescAndPut(fBlock, blockSz, slabAligned);
    bkndSync.blockReleased();
}

void AllLargeBlocksList::add(LargeMemoryBlock *lmb)
{
    MallocMutex::scoped_lock scoped_cs(largeObjLock);
    lmb->gPrev = nullptr;
    lmb->gNext = loHead;
    if (lmb->gNext)
        lmb->gNext->gPrev = lmb;
    loHead = lmb;
}

void AllLargeBlocksList::remove(LargeMemoryBlock *lmb)
{
    MallocMutex::scoped_lock scoped_cs(largeObjLock);
    if (loHead == lmb)
        loHead = lmb->gNext;
    if (lmb->gNext)
        lmb->gNext->gPrev = lmb->gPrev;
    if (lmb->gPrev)
        lmb->gPrev->gNext = lmb->gNext;
}

void Backend::putLargeBlock(LargeMemoryBlock *lmb)
{
    if (extMemPool->userPool())
        extMemPool->lmbList.remove(lmb);
    genericPutBlock((FreeBlock *)lmb, lmb->unalignedSize, false);
}

void Backend::returnLargeObject(LargeMemoryBlock *lmb)
{
    removeBackRef(lmb->backRefIdx);
    putLargeBlock(lmb);
    STAT_increment(getThreadId(), ThreadCommonCounters, freeLargeObj);
}

#if BACKEND_HAS_MREMAP
void *Backend::remap(void *ptr, size_t oldSize, size_t newSize, size_t alignment)
{
    // no remap for user pools and for object too small that living in bins
    if (inUserPool() || min(oldSize, newSize)<maxBinned_SmallPage
        // during remap, can't guarantee alignment more strict than current or
        // more strict than page alignment
        || !isAligned(ptr, alignment) || alignment>extMemPool->granularity)
        return nullptr;
    const LargeMemoryBlock* lmbOld = ((LargeObjectHdr *)ptr - 1)->memoryBlock;
    const size_t oldUnalignedSize = lmbOld->unalignedSize;
    FreeBlock *oldFBlock = (FreeBlock *)lmbOld;
    FreeBlock *right = oldFBlock->rightNeig(oldUnalignedSize);
    // in every region only one block can have LAST_REGION_BLOCK on right,
    // so don't need no synchronization
    if (!right->isLastRegionBlock())
        return nullptr;

    MemRegion *oldRegion = static_cast<LastFreeBlock*>(right)->memRegion;
    MALLOC_ASSERT( oldRegion < ptr, ASSERT_TEXT );
    const size_t oldRegionSize = oldRegion->allocSz;
    if (oldRegion->type != MEMREG_ONE_BLOCK)
        return nullptr;  // we are not single in the region
    const size_t userOffset = (uintptr_t)ptr - (uintptr_t)oldRegion;
    const size_t alignedSize = LargeObjectCache::alignToBin(newSize + userOffset);
    const size_t requestSize =
        alignUp(sizeof(MemRegion) + alignedSize + sizeof(LastFreeBlock), extMemPool->granularity);
    if (requestSize < alignedSize) // is wrapped around?
        return nullptr;
    regionList.remove(oldRegion);

    // The deallocation should be registered in address range before mremap to
    // prevent a race condition with allocation on another thread.
    // (OS can reuse the memory and registerAlloc will be missed on another thread)
    usedAddrRange.registerFree((uintptr_t)oldRegion, (uintptr_t)oldRegion + oldRegionSize);

    void *ret = mremap(oldRegion, oldRegion->allocSz, requestSize, MREMAP_MAYMOVE);
    if (MAP_FAILED == ret) { // can't remap, revert and leave
        regionList.add(oldRegion);
        usedAddrRange.registerAlloc((uintptr_t)oldRegion, (uintptr_t)oldRegion + oldRegionSize);
        return nullptr;
    }
    MemRegion *region = (MemRegion*)ret;
    MALLOC_ASSERT(region->type == MEMREG_ONE_BLOCK, ASSERT_TEXT);
    region->allocSz = requestSize;
    region->blockSz = alignedSize;

    FreeBlock *fBlock = (FreeBlock *)alignUp((uintptr_t)region + sizeof(MemRegion),
                                             largeObjectAlignment);

    regionList.add(region);
    startUseBlock(region, fBlock, /*addToBin=*/false);
    MALLOC_ASSERT(fBlock->sizeTmp == region->blockSz, ASSERT_TEXT);
    // matched blockConsumed() in startUseBlock().
    // TODO: get rid of useless pair blockConsumed()/blockReleased()
    bkndSync.blockReleased();

    // object must start at same offset from region's start
    void *object = (void*)((uintptr_t)region + userOffset);
    MALLOC_ASSERT(isAligned(object, alignment), ASSERT_TEXT);
    LargeObjectHdr *header = (LargeObjectHdr*)object - 1;
    setBackRef(header->backRefIdx, header);

    LargeMemoryBlock *lmb = (LargeMemoryBlock*)fBlock;
    lmb->unalignedSize = region->blockSz;
    lmb->objectSize = newSize;
    lmb->backRefIdx = header->backRefIdx;
    header->memoryBlock = lmb;
    MALLOC_ASSERT((uintptr_t)lmb + lmb->unalignedSize >=
                  (uintptr_t)object + lmb->objectSize, "An object must fit to the block.");

    usedAddrRange.registerAlloc((uintptr_t)region, (uintptr_t)region + requestSize);
    totalMemSize.fetch_add(region->allocSz - oldRegionSize);

    return object;
}
#endif /* BACKEND_HAS_MREMAP */

void Backend::releaseRegion(MemRegion *memRegion)
{
    regionList.remove(memRegion);
    freeRawMem(memRegion, memRegion->allocSz);
}

// coalesce fBlock with its neighborhood
FreeBlock *Backend::doCoalesc(FreeBlock *fBlock, MemRegion **mRegion)
{
    FreeBlock *resBlock = fBlock;
    size_t resSize = fBlock->sizeTmp;
    MemRegion *memRegion = nullptr;

    fBlock->markCoalescing(resSize);
    resBlock->blockInBin = false;

    // coalescing with left neighbor
    size_t leftSz = fBlock->trySetLeftUsed(GuardedSize::COAL_BLOCK);
    if (leftSz != GuardedSize::LOCKED) {
        if (leftSz == GuardedSize::COAL_BLOCK) {
            coalescQ.putBlock(fBlock);
            return nullptr;
        } else {
            FreeBlock *left = fBlock->leftNeig(leftSz);
            size_t lSz = left->trySetMeUsed(GuardedSize::COAL_BLOCK);
            if (lSz <= GuardedSize::MAX_LOCKED_VAL) {
                fBlock->setLeftFree(leftSz); // rollback
                coalescQ.putBlock(fBlock);
                return nullptr;
            } else {
                MALLOC_ASSERT(lSz == leftSz, "Invalid header");
                left->blockInBin = true;
                resBlock = left;
                resSize += leftSz;
                resBlock->sizeTmp = resSize;
            }
        }
    }
    // coalescing with right neighbor
    FreeBlock *right = fBlock->rightNeig(fBlock->sizeTmp);
    size_t rightSz = right->trySetMeUsed(GuardedSize::COAL_BLOCK);
    if (rightSz != GuardedSize::LOCKED) {
        // LastFreeBlock is on the right side
        if (GuardedSize::LAST_REGION_BLOCK == rightSz) {
            right->setMeFree(GuardedSize::LAST_REGION_BLOCK);
            memRegion = static_cast<LastFreeBlock*>(right)->memRegion;
        } else if (GuardedSize::COAL_BLOCK == rightSz) {
            if (resBlock->blockInBin) {
                resBlock->blockInBin = false;
                removeBlockFromBin(resBlock);
            }
            coalescQ.putBlock(resBlock);
            return nullptr;
        } else {
            size_t rSz = right->rightNeig(rightSz)->
                trySetLeftUsed(GuardedSize::COAL_BLOCK);
            if (rSz <= GuardedSize::MAX_LOCKED_VAL) {
                right->setMeFree(rightSz);  // rollback
                if (resBlock->blockInBin) {
                    resBlock->blockInBin = false;
                    removeBlockFromBin(resBlock);
                }
                coalescQ.putBlock(resBlock);
                return nullptr;
            } else {
                MALLOC_ASSERT(rSz == rightSz, "Invalid header");
                removeBlockFromBin(right);
                resSize += rightSz;

                // Is LastFreeBlock on the right side of right?
                FreeBlock *nextRight = right->rightNeig(rightSz);
                size_t nextRightSz = nextRight->
                    trySetMeUsed(GuardedSize::COAL_BLOCK);
                if (nextRightSz > GuardedSize::MAX_LOCKED_VAL) {
                    if (nextRightSz == GuardedSize::LAST_REGION_BLOCK)
                        memRegion = static_cast<LastFreeBlock*>(nextRight)->memRegion;

                    nextRight->setMeFree(nextRightSz);
                }
            }
        }
    }
    if (memRegion) {
        MALLOC_ASSERT((uintptr_t)memRegion + memRegion->allocSz >=
                      (uintptr_t)right + sizeof(LastFreeBlock), ASSERT_TEXT);
        MALLOC_ASSERT((uintptr_t)memRegion < (uintptr_t)resBlock, ASSERT_TEXT);
        *mRegion = memRegion;
    } else
        *mRegion = nullptr;
    resBlock->sizeTmp = resSize;
    return resBlock;
}

bool Backend::coalescAndPutList(FreeBlock *list, bool forceCoalescQDrop, bool reportBlocksProcessed)
{
    bool regionReleased = false;

    for (FreeBlock *helper; list;
         list = helper,
             // matches block enqueue in CoalRequestQ::putBlock()
             reportBlocksProcessed? coalescQ.blockWasProcessed() : (void)0) {
        MemRegion *memRegion;
        bool addToTail = false;

        helper = list->nextToFree;
        FreeBlock *toRet = doCoalesc(list, &memRegion);
        if (!toRet)
            continue;

        if (memRegion && memRegion->blockSz == toRet->sizeTmp
            && !extMemPool->fixedPool) {
            if (extMemPool->regionsAreReleaseable()) {
                // release the region, because there is no used blocks in it
                if (toRet->blockInBin)
                    removeBlockFromBin(toRet);
                releaseRegion(memRegion);
                regionReleased = true;
                continue;
            } else // add block from empty region to end of bin,
                addToTail = true; // preserving for exact fit
        }
        size_t currSz = toRet->sizeTmp;
        int bin = sizeToBin(currSz);
        bool toAligned = extMemPool->fixedPool ? toAlignedBin(toRet, currSz) : toRet->slabAligned;
        bool needAddToBin = true;

        if (toRet->blockInBin) {
            // Does it stay in same bin?
            if (toRet->myBin == bin && toRet->slabAligned == toAligned)
                needAddToBin = false;
            else {
                toRet->blockInBin = false;
                removeBlockFromBin(toRet);
            }
        }

        // Does not stay in same bin, or bin-less; add it
        if (needAddToBin) {
            toRet->prev = toRet->next = toRet->nextToFree = nullptr;
            toRet->myBin = NO_BIN;
            toRet->slabAligned = toAligned;

            // If the block is too small to fit in any bin, keep it bin-less.
            // It's not a leak because the block later can be coalesced.
            if (currSz >= minBinnedSize) {
                toRet->sizeTmp = currSz;
                IndexedBins *target = toRet->slabAligned ? &freeSlabAlignedBins : &freeLargeBlockBins;
                if (forceCoalescQDrop) {
                    target->addBlock(bin, toRet, toRet->sizeTmp, addToTail);
                } else if (!target->tryAddBlock(bin, toRet, addToTail)) {
                    coalescQ.putBlock(toRet);
                    continue;
                }
            }
            toRet->sizeTmp = 0;
        }
        // Free (possibly coalesced) free block.
        // Adding to bin must be done before this point,
        // because after a block is free it can be coalesced, and
        // using its pointer became unsafe.
        // Remember that coalescing is not done under any global lock.
        toRet->setMeFree(currSz);
        toRet->rightNeig(currSz)->setLeftFree(currSz);
    }
    return regionReleased;
}

// Coalesce fBlock and add it back to a bin;
// processing delayed coalescing requests.
void Backend::coalescAndPut(FreeBlock *fBlock, size_t blockSz, bool slabAligned)
{
    fBlock->sizeTmp = blockSz;
    fBlock->nextToFree = nullptr;
    fBlock->slabAligned = slabAligned;

    coalescAndPutList(fBlock, /*forceCoalescQDrop=*/false, /*reportBlocksProcessed=*/false);
}

bool Backend::scanCoalescQ(bool forceCoalescQDrop)
{
    FreeBlock *currCoalescList = coalescQ.getAll();

    if (currCoalescList)
        // reportBlocksProcessed=true informs that the blocks leave coalescQ,
        // matches blockConsumed() from CoalRequestQ::putBlock()
        coalescAndPutList(currCoalescList, forceCoalescQDrop,
                          /*reportBlocksProcessed=*/true);
    // returns status of coalescQ.getAll(), as an indication of possible changes in backend
    // TODO: coalescAndPutList() may report is some new free blocks became available or not
    return currCoalescList;
}

FreeBlock *Backend::findBlockInRegion(MemRegion *region, size_t exactBlockSize)
{
    FreeBlock *fBlock;
    size_t blockSz;
    uintptr_t fBlockEnd,
        lastFreeBlock = (uintptr_t)region + region->allocSz - sizeof(LastFreeBlock);

    static_assert(sizeof(LastFreeBlock) % sizeof(uintptr_t) == 0,
        "Atomic applied on LastFreeBlock, and we put it at the end of region, that"
        " is uintptr_t-aligned, so no unaligned atomic operations are possible.");
     // right bound is slab-aligned, keep LastFreeBlock after it
    if (region->type == MEMREG_SLAB_BLOCKS) {
        fBlock = (FreeBlock *)alignUp((uintptr_t)region + sizeof(MemRegion), sizeof(uintptr_t));
        fBlockEnd = alignDown(lastFreeBlock, slabSize);
    } else {
        fBlock = (FreeBlock *)alignUp((uintptr_t)region + sizeof(MemRegion), largeObjectAlignment);
        fBlockEnd = (uintptr_t)fBlock + exactBlockSize;
        MALLOC_ASSERT(fBlockEnd <= lastFreeBlock, ASSERT_TEXT);
    }
    if (fBlockEnd <= (uintptr_t)fBlock)
        return nullptr; // allocSz is too small
    blockSz = fBlockEnd - (uintptr_t)fBlock;
    // TODO: extend getSlabBlock to support degradation, i.e. getting less blocks
    // then requested, and then relax this check
    // (now all or nothing is implemented, check according to this)
    if (blockSz < numOfSlabAllocOnMiss*slabSize)
        return nullptr;

    region->blockSz = blockSz;
    return fBlock;
}

// startUseBlock may add the free block to a bin, the block can be used and
// even released after this, so the region must be added to regionList already
void Backend::startUseBlock(MemRegion *region, FreeBlock *fBlock, bool addToBin)
{
    size_t blockSz = region->blockSz;
    fBlock->initHeader();
    fBlock->setMeFree(blockSz);

    LastFreeBlock *lastBl = static_cast<LastFreeBlock*>(fBlock->rightNeig(blockSz));
    // to not get unaligned atomics during LastFreeBlock access
    MALLOC_ASSERT(isAligned(lastBl, sizeof(uintptr_t)), nullptr);
    lastBl->initHeader();
    lastBl->setMeFree(GuardedSize::LAST_REGION_BLOCK);
    lastBl->setLeftFree(blockSz);
    lastBl->myBin = NO_BIN;
    lastBl->memRegion = region;

    if (addToBin) {
        unsigned targetBin = sizeToBin(blockSz);
        // during adding advance regions, register bin for a largest block in region
        advRegBins.registerBin(targetBin);
        if (region->type == MEMREG_SLAB_BLOCKS) {
            fBlock->slabAligned = true;
            freeSlabAlignedBins.addBlock(targetBin, fBlock, blockSz, /*addToTail=*/false);
        } else {
            fBlock->slabAligned = false;
            freeLargeBlockBins.addBlock(targetBin, fBlock, blockSz, /*addToTail=*/false);
        }
    } else {
        // to match with blockReleased() in genericGetBlock
        bkndSync.blockConsumed();
        // Understand our alignment for correct splitBlock operation
        fBlock->slabAligned = region->type == MEMREG_SLAB_BLOCKS ? true : false;
        fBlock->sizeTmp = fBlock->tryLockBlock();
        MALLOC_ASSERT(fBlock->sizeTmp >= FreeBlock::minBlockSize, "Locking must be successful");
    }
}

void MemRegionList::add(MemRegion *r)
{
    r->prev = nullptr;
    MallocMutex::scoped_lock lock(regionListLock);
    r->next = head;
    head = r;
    if (head->next)
        head->next->prev = head;
}

void MemRegionList::remove(MemRegion *r)
{
    MallocMutex::scoped_lock lock(regionListLock);
    if (head == r)
        head = head->next;
    if (r->next)
        r->next->prev = r->prev;
    if (r->prev)
        r->prev->next = r->next;
}

#if __TBB_MALLOC_BACKEND_STAT
int MemRegionList::reportStat(FILE *f)
{
    int regNum = 0;
    MallocMutex::scoped_lock lock(regionListLock);
    for (MemRegion *curr = head; curr; curr = curr->next) {
        fprintf(f, "%p: max block %lu B, ", curr, curr->blockSz);
        regNum++;
    }
    return regNum;
}
#endif

FreeBlock *Backend::addNewRegion(size_t size, MemRegionType memRegType, bool addToBin)
{
    static_assert(sizeof(BlockMutexes) <= sizeof(BlockI), "Header must be not overwritten in used blocks");
    MALLOC_ASSERT(FreeBlock::minBlockSize > GuardedSize::MAX_SPEC_VAL,
          "Block length must not conflict with special values of GuardedSize");
    // If the region is not "for slabs" we should reserve some space for
    // a region header, the worst case alignment and the last block mark.
    const size_t requestSize = memRegType == MEMREG_SLAB_BLOCKS ? size :
        size + sizeof(MemRegion) + largeObjectAlignment
             +  FreeBlock::minBlockSize + sizeof(LastFreeBlock);

    size_t rawSize = requestSize;
    MemRegion *region = (MemRegion*)allocRawMem(rawSize);
    if (!region) {
        MALLOC_ASSERT(rawSize==requestSize, "getRawMem has not allocated memory but changed the allocated size.");
        return nullptr;
    }
    if (rawSize < sizeof(MemRegion)) {
        if (!extMemPool->fixedPool)
            freeRawMem(region, rawSize);
        return nullptr;
    }

    region->type = memRegType;
    region->allocSz = rawSize;
    FreeBlock *fBlock = findBlockInRegion(region, size);
    if (!fBlock) {
        if (!extMemPool->fixedPool)
            freeRawMem(region, rawSize);
        return nullptr;
    }
    regionList.add(region);
    startUseBlock(region, fBlock, addToBin);
    bkndSync.binsModified();
    return addToBin? (FreeBlock*)VALID_BLOCK_IN_BIN : fBlock;
}

void Backend::init(ExtMemoryPool *extMemoryPool)
{
    extMemPool = extMemoryPool;
    usedAddrRange.init();
    coalescQ.init(&bkndSync);
    bkndSync.init(this);
}

void Backend::reset()
{
    MALLOC_ASSERT(extMemPool->userPool(), "Only user pool can be reset.");
    // no active threads are allowed in backend while reset() called
    verify();

    freeLargeBlockBins.reset();
    freeSlabAlignedBins.reset();
    advRegBins.reset();

    for (MemRegion *curr = regionList.head; curr; curr = curr->next) {
        FreeBlock *fBlock = findBlockInRegion(curr, curr->blockSz);
        MALLOC_ASSERT(fBlock, "A memory region unexpectedly got smaller");
        startUseBlock(curr, fBlock, /*addToBin=*/true);
    }
}

bool Backend::destroy()
{
    bool noError = true;
    // no active threads are allowed in backend while destroy() called
    verify();
    if (!inUserPool()) {
        freeLargeBlockBins.reset();
        freeSlabAlignedBins.reset();
    }
    while (regionList.head) {
        MemRegion *helper = regionList.head->next;
        noError &= freeRawMem(regionList.head, regionList.head->allocSz);
        regionList.head = helper;
    }
    return noError;
}

bool Backend::clean()
{
    scanCoalescQ(/*forceCoalescQDrop=*/false);

    bool res = false;
    // We can have several blocks occupying a whole region,
    // because such regions are added in advance (see askMemFromOS() and reset()),
    // and never used. Release them all.
    for (int i = advRegBins.getMinUsedBin(0); i != -1; i = advRegBins.getMinUsedBin(i+1)) {
        if (i == freeSlabAlignedBins.getMinNonemptyBin(i))
            res |= freeSlabAlignedBins.tryReleaseRegions(i, this);
        if (i == freeLargeBlockBins.getMinNonemptyBin(i))
            res |= freeLargeBlockBins.tryReleaseRegions(i, this);
    }

    return res;
}

void Backend::IndexedBins::verify()
{
#if MALLOC_DEBUG
    for (int i=0; i<freeBinsNum; i++) {
        for (FreeBlock *fb = freeBins[i].head.load(std::memory_order_relaxed); fb; fb=fb->next) {
            uintptr_t mySz = fb->myL.value;
            MALLOC_ASSERT(mySz>GuardedSize::MAX_SPEC_VAL, ASSERT_TEXT);
            FreeBlock *right = (FreeBlock*)((uintptr_t)fb + mySz);
            suppress_unused_warning(right);
            MALLOC_ASSERT(right->myL.value<=GuardedSize::MAX_SPEC_VAL, ASSERT_TEXT);
            MALLOC_ASSERT(right->leftL.value==mySz, ASSERT_TEXT);
            MALLOC_ASSERT(fb->leftL.value<=GuardedSize::MAX_SPEC_VAL, ASSERT_TEXT);
        }
    }
#endif
}

// For correct operation, it must be called when no other threads
// is changing backend.
void Backend::verify()
{
#if MALLOC_DEBUG
    scanCoalescQ(/*forceCoalescQDrop=*/false);
#endif // MALLOC_DEBUG

    freeLargeBlockBins.verify();
    freeSlabAlignedBins.verify();
}

#if __TBB_MALLOC_BACKEND_STAT
size_t Backend::Bin::countFreeBlocks()
{
    size_t cnt = 0;
    {
        MallocMutex::scoped_lock lock(tLock);
        for (FreeBlock *fb = head; fb; fb = fb->next)
            cnt++;
    }
    return cnt;
}

size_t Backend::Bin::reportFreeBlocks(FILE *f)
{
    size_t totalSz = 0;
    MallocMutex::scoped_lock lock(tLock);
    for (FreeBlock *fb = head; fb; fb = fb->next) {
        size_t sz = fb->tryLockBlock();
        fb->setMeFree(sz);
        fprintf(f, " [%p;%p]", fb, (void*)((uintptr_t)fb+sz));
        totalSz += sz;
    }
    return totalSz;
}

void Backend::IndexedBins::reportStat(FILE *f)
{
    size_t totalSize = 0;

    for (int i=0; i<Backend::freeBinsNum; i++)
        if (size_t cnt = freeBins[i].countFreeBlocks()) {
            totalSize += freeBins[i].reportFreeBlocks(f);
            fprintf(f, " %d:%lu, ", i, cnt);
        }
    fprintf(f, "\ttotal size %lu KB", totalSize/1024);
}

void Backend::reportStat(FILE *f)
{
    scanCoalescQ(/*forceCoalescQDrop=*/false);

    fprintf(f, "\n  regions:\n");
    int regNum = regionList.reportStat(f);
    fprintf(f, "\n%d regions, %lu KB in all regions\n  free bins:\nlarge bins: ",
            regNum, totalMemSize/1024);
    freeLargeBlockBins.reportStat(f);
    fprintf(f, "\naligned bins: ");
    freeSlabAlignedBins.reportStat(f);
    fprintf(f, "\n");
}
#endif // __TBB_MALLOC_BACKEND_STAT

} } // namespaces
