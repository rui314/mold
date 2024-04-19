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

//! \file test_malloc_compliance.cpp
//! \brief Test for [memory_allocation.scalable_alloc_c_interface] functionality

#define __TBB_NO_IMPLICIT_LINKAGE 1

#define __STDC_LIMIT_MACROS 1 // to get SIZE_MAX from stdint.h

#include "common/test.h"

#include "common/utils.h"
#include "common/utils_report.h"
#include "common/spin_barrier.h"
#include "common/memory_usage.h"

#include "oneapi/tbb/detail/_config.h"

#define __TBB_NO_IMPLICIT_LINKAGE 1
#include "tbb/scalable_allocator.h"

#include <vector>

#if _WIN32 || _WIN64
/**
 *  _WIN32_WINNT should be defined at the very beginning,
 *  because other headers might include <windows.h>
 **/
#undef _WIN32_WINNT
#define _WIN32_WINNT 0x0501
#include <windows.h>
#include <stdio.h>

#if _MSC_VER && defined(_MT) && defined(_DLL)
    #pragma comment(lib, "version.lib")  // to use GetFileVersionInfo*
#endif

void limitMem( size_t limit )
{
    static HANDLE hJob = nullptr;
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION jobInfo;

    jobInfo.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    jobInfo.ProcessMemoryLimit = limit? limit*MByte : 2*MByte*1024;
    if (nullptr == hJob) {
        if (nullptr == (hJob = CreateJobObject(nullptr, nullptr))) {
            REPORT("Can't assign create job object: %ld\n", GetLastError());
            exit(1);
        }
        if (0 == AssignProcessToJobObject(hJob, GetCurrentProcess())) {
            REPORT("Can't assign process to job object: %ld\n", GetLastError());
            exit(1);
        }
    }
    if (0 == SetInformationJobObject(hJob, JobObjectExtendedLimitInformation,
                                     &jobInfo, sizeof(jobInfo))) {
        REPORT("Can't set limits: %ld\n", GetLastError());
        exit(1);
    }
}
// Do not test errno with static VC runtime
#else // _WIN32 || _WIN64
#include <sys/resource.h>
#include <stdlib.h>
#include <stdio.h>
#include <errno.h>
#include <sys/types.h>  // uint64_t on FreeBSD, needed for rlim_t
#include <stdint.h>     // SIZE_MAX

void limitMem( size_t limit )
{
    rlimit rlim;
    int ret = getrlimit(RLIMIT_AS,&rlim);
    if (0 != ret) {
        REPORT("getrlimit() returned an error: errno %d\n", errno);
        exit(1);
    }
    if (rlim.rlim_max==(rlim_t)RLIM_INFINITY)
        rlim.rlim_cur = (limit > 0) ? limit*MByte : rlim.rlim_max;
    else rlim.rlim_cur = (limit > 0 && static_cast<rlim_t>(limit)<rlim.rlim_max) ? limit*MByte : rlim.rlim_max;
    ret = setrlimit(RLIMIT_AS,&rlim);
    if (0 != ret) {
        REPORT("Can't set limits: errno %d\n", errno);
        exit(1);
    }
}
#endif  // _WIN32 || _WIN64

bool __tbb_test_errno = false;

#define ASSERT_ERRNO(cond, msg)  REQUIRE_MESSAGE( (!__tbb_test_errno || (cond)), msg )
#define CHECK_ERRNO(cond) (__tbb_test_errno && (cond))

static const int MinThread = 1;
static const int MaxThread = 4;
static bool Verbose = false;

#include <time.h>
#include <errno.h>
#include <limits.h> // for CHAR_BIT

#if __unix__
#include <stdint.h> // uintptr_t
#endif
#if _WIN32 || _WIN64
#include <malloc.h> // _aligned_(malloc|free|realloc)
#if __MINGW64__
// Workaround a bug in MinGW64 headers with _aligned_(malloc|free) not declared by default
extern "C" void __cdecl __declspec(dllimport) _aligned_free(void *);
extern "C" void *__cdecl __declspec(dllimport) _aligned_malloc(size_t,size_t);
#endif
#endif

#include <vector>

const int COUNT_ELEM = 25000;
const size_t MAX_SIZE = 1000;
const int COUNTEXPERIMENT = 10000;

const char strError[]="failed";
const char strOk[]="done";

typedef unsigned int UINT;
typedef unsigned char UCHAR;
typedef unsigned long DWORD;
typedef unsigned char BYTE;

typedef void* TestMalloc(size_t size);
typedef void* TestCalloc(size_t num, size_t size);
typedef void* TestRealloc(void* memblock, size_t size);
typedef void  TestFree(void* memblock);
typedef int   TestPosixMemalign(void **memptr, size_t alignment, size_t size);
typedef void* TestAlignedMalloc(size_t size, size_t alignment);
typedef void* TestAlignedRealloc(void* memblock, size_t size, size_t alignment);
typedef void  TestAlignedFree(void* memblock);

// pointers to tested functions
TestMalloc*  Rmalloc;
TestCalloc*  Rcalloc;
TestRealloc* Rrealloc;
TestFree*    Tfree;
TestPosixMemalign*  Rposix_memalign;
TestAlignedMalloc*  Raligned_malloc;
TestAlignedRealloc* Raligned_realloc;
TestAlignedFree* Taligned_free;

// call functions via pointer and check result's alignment
void* Tmalloc(size_t size);
void* Tcalloc(size_t num, size_t size);
void* Trealloc(void* memblock, size_t size);
int   Tposix_memalign(void **memptr, size_t alignment, size_t size);
void* Taligned_malloc(size_t size, size_t alignment);
void* Taligned_realloc(void* memblock, size_t size, size_t alignment);

std::atomic<bool> error_occurred{ false };

#if __APPLE__
// Tests that use the variables are skipped on macOS*
#else
const size_t COUNT_ELEM_CALLOC = 2;
const int COUNT_TESTS = 1000;
static bool perProcessLimits = true;
#endif

const size_t POWERS_OF_2 = 20;

struct MemStruct
{
    void* Pointer;
    UINT Size;

    MemStruct() : Pointer(nullptr), Size(0) {}
    MemStruct(void* ptr, UINT sz) : Pointer(ptr), Size(sz) {}
};

class CMemTest: utils::NoAssign
{
    bool FullLog;
    utils::SpinBarrier *limitBarrier;
    static bool firstTime;

public:
    CMemTest(utils::SpinBarrier *barrier, bool isVerbose=false) : limitBarrier(barrier)
        {
            srand((UINT)time(nullptr));
            FullLog=isVerbose;
        }
    void NULLReturn(UINT MinSize, UINT MaxSize, int total_threads); // nullptr pointer + check errno
    void UniquePointer(); // unique pointer - check with padding
    void AddrArifm(); // unique pointer - check with pointer arithmetic
    bool ShouldReportError();
    void Free_NULL(); //
    void Zerofilling(); // check if arrays are zero-filled
    void TestAlignedParameters();
    void RunAllTests(int total_threads);
    ~CMemTest() {}
};

class Limit {
    size_t limit;
public:
    Limit(size_t a_limit) : limit(a_limit) {}
    void operator() () const {
        limitMem(limit);
    }
};

int argC;
char** argV;

struct RoundRobin: utils::NoAssign {
    const long number_of_threads;
    mutable CMemTest test;

    RoundRobin( long p, utils::SpinBarrier *limitBarrier, bool verbose ) :
        number_of_threads(p), test(limitBarrier, verbose) {}
    void operator()( int /*id*/ ) const
        {
            test.RunAllTests(number_of_threads);
        }
};

bool CMemTest::firstTime = true;

inline size_t choose_random_alignment() {
    return sizeof(void*)<<(rand() % POWERS_OF_2);
}

#if TBB_REVAMP_TODO
// TODO: enable this test mode
static void setSystemAllocs()
{
    Rmalloc=malloc;
    Rrealloc=realloc;
    Rcalloc=calloc;
    Tfree=free;
#if _WIN32 || _WIN64
    Raligned_malloc=_aligned_malloc;
    Raligned_realloc=_aligned_realloc;
    Taligned_free=_aligned_free;
    Rposix_memalign=0;
#elif  __APPLE__ || __sun || __ANDROID__
// macOS, Solaris*, and Android* don't have posix_memalign
    Raligned_malloc=0;
    Raligned_realloc=0;
    Taligned_free=0;
    Rposix_memalign=0;
#else
    Raligned_malloc=0;
    Raligned_realloc=0;
    Taligned_free=0;
    Rposix_memalign=posix_memalign;
#endif
}
#endif

// check that realloc works as free and as malloc
void ReallocParam()
{
    const int ITERS = 1000;
    int i;
    void *bufs[ITERS];

    bufs[0] = Trealloc(nullptr, 30*MByte);
    REQUIRE_MESSAGE(bufs[0], "Can't get memory to start the test.");

    for (i=1; i<ITERS; i++)
    {
        bufs[i] = Trealloc(nullptr, 30*MByte);
        if (nullptr == bufs[i])
            break;
    }
    REQUIRE_MESSAGE(i<ITERS, "Limits should be decreased for the test to work.");

    Trealloc(bufs[0], 0);
    /* There is a race for the free space between different threads at
       this point. So, have to run the test sequentially.
    */
    bufs[0] = Trealloc(nullptr, 30*MByte);
    REQUIRE(bufs[0]);

    for (int j=0; j<i; j++)
        Trealloc(bufs[j], 0);
}

void CheckArgumentsOverflow()
{
    void *p;
    const size_t params[] = {SIZE_MAX, SIZE_MAX-16};

    for (unsigned i=0; i<utils::array_length(params); i++) {
        p = Tmalloc(params[i]);
        REQUIRE(!p);
        ASSERT_ERRNO(errno==ENOMEM, nullptr);
        p = Trealloc(nullptr, params[i]);
        REQUIRE(!p);
        ASSERT_ERRNO(errno==ENOMEM, nullptr);
        p = Tcalloc(1, params[i]);
        REQUIRE(!p);
        ASSERT_ERRNO(errno==ENOMEM, nullptr);
        p = Tcalloc(params[i], 1);
        REQUIRE(!p);
        ASSERT_ERRNO(errno==ENOMEM, nullptr);
    }
    const size_t max_alignment = size_t(1) << (sizeof(size_t)*CHAR_BIT - 1);
    if (Rposix_memalign) {
        int ret = Rposix_memalign(&p, max_alignment, ~max_alignment);
        REQUIRE(ret == ENOMEM);
        for (unsigned i=0; i<utils::array_length(params); i++) {
            ret = Rposix_memalign(&p, max_alignment, params[i]);
            REQUIRE(ret == ENOMEM);
            ret = Rposix_memalign(&p, sizeof(void*), params[i]);
            REQUIRE(ret == ENOMEM);
        }
    }
    if (Raligned_malloc) {
        p = Raligned_malloc(~max_alignment, max_alignment);
        REQUIRE(!p);
        for (unsigned i=0; i<utils::array_length(params); i++) {
            p = Raligned_malloc(params[i], max_alignment);
            REQUIRE(!p);
            ASSERT_ERRNO(errno==ENOMEM, nullptr);
            p = Raligned_malloc(params[i], sizeof(void*));
            REQUIRE(!p);
            ASSERT_ERRNO(errno==ENOMEM, nullptr);
        }
    }

    p = Tcalloc(SIZE_MAX/2-16, SIZE_MAX/2-16);
    REQUIRE(!p);
    ASSERT_ERRNO(errno==ENOMEM, nullptr);
    p = Tcalloc(SIZE_MAX/2, SIZE_MAX/2);
    REQUIRE(!p);
    ASSERT_ERRNO(errno==ENOMEM, nullptr);
}

void InvariantDataRealloc(bool aligned, size_t maxAllocSize, bool checkData)
{
    utils::FastRandom<> fastRandom(1);
    size_t size = 0, start = 0;
    char *ptr = nullptr,
        // external thread to create copies and compare ralloc result against it
        *base = (char*)Tmalloc(2*maxAllocSize);

    REQUIRE(base);
    REQUIRE_MESSAGE(!(2*maxAllocSize%sizeof(unsigned short)),
           "The loop below expects that 2*maxAllocSize contains sizeof(unsigned short)");
    for (size_t k = 0; k<2*maxAllocSize; k+=sizeof(unsigned short))
        *(unsigned short*)(base+k) = fastRandom.get();

    for (int i=0; i<100; i++) {
        // don't want sizeNew==0 here
        const size_t sizeNew = fastRandom.get() % (maxAllocSize-1) + 1;
        char *ptrNew = aligned?
            (char*)Taligned_realloc(ptr, sizeNew, choose_random_alignment())
            : (char*)Trealloc(ptr, sizeNew);
        REQUIRE(ptrNew);
        // check that old data not changed
        if (checkData)
            REQUIRE_MESSAGE(!memcmp(ptrNew, base+start, utils::min(size, sizeNew)), "broken data");

        // prepare fresh data, copying them from random position in external
        size = sizeNew;
        ptr = ptrNew;
        if (checkData) {
            start = fastRandom.get() % maxAllocSize;
            memcpy(ptr, base+start, size);
        }
    }
    if (aligned)
        Taligned_realloc(ptr, 0, choose_random_alignment());
    else
        Trealloc(ptr, 0);
    Tfree(base);
}

void CheckReallocLeak()
{
    int i;
    const int ITER_TO_STABILITY = 10;
    // do bootstrap
    for (int k=0; k<3; k++)
        InvariantDataRealloc(/*aligned=*/false, 128*MByte, /*checkData=*/false);
    size_t prev = utils::GetMemoryUsage(utils::peakUsage);
    // expect realloc to not increase peak memory consumption after ITER_TO_STABILITY-1 iterations
    for (i=0; i<ITER_TO_STABILITY; i++) {
        for (int k=0; k<3; k++)
            InvariantDataRealloc(/*aligned=*/false, 128*MByte, /*checkData=*/false);
        size_t curr = utils::GetMemoryUsage(utils::peakUsage);
        if (prev == curr)
            break;
        prev = curr;
    }
    REQUIRE_MESSAGE(i < ITER_TO_STABILITY, "Can't stabilize memory consumption.");
}

// if non-zero byte found, returns bad value address plus 1
size_t NonZero(void *ptr, size_t size)
{
    size_t words = size / sizeof(intptr_t);
    size_t tailSz = size % sizeof(intptr_t);
    intptr_t *buf =(intptr_t*)ptr;
    char *bufTail =(char*)(buf+words);

    for (size_t i=0; i<words; i++)
        if (buf[i]) {
            for (unsigned b=0; b<sizeof(intptr_t); b++)
                if (((char*)(buf+i))[b])
                    return sizeof(intptr_t)*i + b + 1;
        }
    for (size_t i=0; i<tailSz; i++)
        if (bufTail[i]) {
            return words*sizeof(intptr_t)+i+1;
        }
    return 0;
}

struct TestStruct
{
    DWORD field1:2;
    DWORD field2:6;
    double field3;
    UCHAR field4[100];
    TestStruct* field5;
    std::vector<int> field7;
    double field8;
};

void* Tmalloc(size_t size)
{
    // For compatibility, on 64-bit systems malloc should align to 16 bytes
    size_t alignment = (sizeof(intptr_t)>4 && size>8) ? 16 : 8;
    void *ret = Rmalloc(size);
    if (nullptr != ret)
        CHECK_FAST_MESSAGE(0==((uintptr_t)ret & (alignment-1)),
               "allocation result should be properly aligned");
    return ret;
}
void* Tcalloc(size_t num, size_t size)
{
    // For compatibility, on 64-bit systems calloc should align to 16 bytes
    size_t alignment = (sizeof(intptr_t)>4 && num && size>8) ? 16 : 8;
    void *ret = Rcalloc(num, size);
    if (nullptr != ret)
        CHECK_FAST_MESSAGE(0==((uintptr_t)ret & (alignment-1)),
               "allocation result should be properly aligned");
    return ret;
}
void* Trealloc(void* memblock, size_t size)
{
    // For compatibility, on 64-bit systems realloc should align to 16 bytes
    size_t alignment = (sizeof(intptr_t)>4 && size>8) ? 16 : 8;
    void *ret = Rrealloc(memblock, size);
    if (nullptr != ret)
        CHECK_FAST_MESSAGE(0==((uintptr_t)ret & (alignment-1)),
               "allocation result should be properly aligned");
    return ret;
}
int Tposix_memalign(void **memptr, size_t alignment, size_t size)
{
    int ret = Rposix_memalign(memptr, alignment, size);
    if (0 == ret)
        CHECK_FAST_MESSAGE(0==((uintptr_t)*memptr & (alignment-1)),
               "allocation result should be aligned");
    return ret;
}
void* Taligned_malloc(size_t size, size_t alignment)
{
    void *ret = Raligned_malloc(size, alignment);
    if (nullptr != ret)
        CHECK_FAST_MESSAGE(0==((uintptr_t)ret & (alignment-1)),
               "allocation result should be aligned");
    return ret;
}
void* Taligned_realloc(void* memblock, size_t size, size_t alignment)
{
    void *ret = Raligned_realloc(memblock, size, alignment);
    if (nullptr != ret)
        CHECK_FAST_MESSAGE(0==((uintptr_t)ret & (alignment-1)),
               "allocation result should be aligned");
    return ret;
}

struct PtrSize {
    void  *ptr;
    size_t size;
};

static int cmpAddrs(const void *p1, const void *p2)
{
    const PtrSize *a = (const PtrSize *)p1;
    const PtrSize *b = (const PtrSize *)p2;

    return a->ptr < b->ptr ? -1 : ( a->ptr == b->ptr ? 0 : 1);
}

void CMemTest::AddrArifm()
{
    PtrSize *arr = (PtrSize*)Tmalloc(COUNT_ELEM*sizeof(PtrSize));

    if (FullLog) REPORT("\nUnique pointer using Address arithmetic\n");
    if (FullLog) REPORT("malloc....");
    REQUIRE(arr);
    for (int i=0; i<COUNT_ELEM; i++)
    {
        arr[i].size=rand()%MAX_SIZE;
        arr[i].ptr=Tmalloc(arr[i].size);
    }
    qsort(arr, COUNT_ELEM, sizeof(PtrSize), cmpAddrs);

    for (int i=0; i<COUNT_ELEM-1; i++)
    {
        if (nullptr!=arr[i].ptr && nullptr!=arr[i+1].ptr)
            REQUIRE_MESSAGE((uintptr_t)arr[i].ptr+arr[i].size <= (uintptr_t)arr[i+1].ptr,
                   "intersection detected");
    }
    //----------------------------------------------------------------
    if (FullLog) REPORT("realloc....");
    for (int i=0; i<COUNT_ELEM; i++)
    {
        size_t count=arr[i].size*2;
        void *tmpAddr=Trealloc(arr[i].ptr,count);
        if (nullptr!=tmpAddr) {
            arr[i].ptr = tmpAddr;
            arr[i].size = count;
        } else if (count==0) { // because realloc(..., 0) works as free
            arr[i].ptr = nullptr;
            arr[i].size = 0;
        }
    }
    qsort(arr, COUNT_ELEM, sizeof(PtrSize), cmpAddrs);

    for (int i=0; i<COUNT_ELEM-1; i++)
    {
        if (nullptr!=arr[i].ptr && nullptr!=arr[i+1].ptr)
            REQUIRE_MESSAGE((uintptr_t)arr[i].ptr+arr[i].size <= (uintptr_t)arr[i+1].ptr,
                   "intersection detected");
    }
    for (int i=0; i<COUNT_ELEM; i++)
    {
        Tfree(arr[i].ptr);
    }
    //-------------------------------------------
    if (FullLog) REPORT("calloc....");
    for (int i=0; i<COUNT_ELEM; i++)
    {
        arr[i].size=rand()%MAX_SIZE;
        arr[i].ptr=Tcalloc(arr[i].size,1);
    }
    qsort(arr, COUNT_ELEM, sizeof(PtrSize), cmpAddrs);

    for (int i=0; i<COUNT_ELEM-1; i++)
    {
        if (nullptr!=arr[i].ptr && nullptr!=arr[i+1].ptr)
            REQUIRE_MESSAGE((uintptr_t)arr[i].ptr+arr[i].size <= (uintptr_t)arr[i+1].ptr,
                   "intersection detected");
    }
    for (int i=0; i<COUNT_ELEM; i++)
    {
        Tfree(arr[i].ptr);
    }
    Tfree(arr);
}

void CMemTest::Zerofilling()
{
    TestStruct* TSMas;
    size_t CountElement;
    static std::atomic<int> CountErrors{0};
    if (FullLog) REPORT("\nzeroings elements of array....");
    //test struct
    for (int i=0; i<COUNTEXPERIMENT; i++)
    {
        CountElement=rand()%MAX_SIZE;
        TSMas=(TestStruct*)Tcalloc(CountElement,sizeof(TestStruct));
        if (nullptr == TSMas)
            continue;
        for (size_t j=0; j<CountElement; j++)
        {
            if (NonZero(TSMas+j, sizeof(TestStruct)))
            {
                CountErrors++;
                if (ShouldReportError()) REPORT("detect nonzero element at TestStruct\n");
            }
        }
        Tfree(TSMas);
    }
    if (CountErrors) REPORT("%s\n",strError);
    else if (FullLog) REPORT("%s\n",strOk);
    if (CountErrors) error_occurred = true;
}

#if !__APPLE__

void myMemset(void *ptr, int c, size_t n)
{
#if  __unix__ &&  __i386__
// memset in Fedora 13 not always correctly sets memory to required values.
    char *p = (char*)ptr;
    for (size_t i=0; i<n; i++)
        p[i] = c;
#else
    memset(ptr, c, n);
#endif
}

// This test requires more than TOTAL_MB_ALLOC MB of RAM.
#if __ANDROID__
// Android requires lower limit due to lack of virtual memory.
#define TOTAL_MB_ALLOC	200
#else
#define TOTAL_MB_ALLOC  800
#endif
void CMemTest::NULLReturn(UINT MinSize, UINT MaxSize, int total_threads)
{
    const int MB_PER_THREAD = TOTAL_MB_ALLOC / total_threads;
    // find size to guarantee getting nullptr for 1024 B allocations
    const int MAXNUM_1024 = (MB_PER_THREAD + (MB_PER_THREAD>>2)) * 1024;

    std::vector<MemStruct> PointerList;
    void *tmp;
    static std::atomic<int> CountErrors{0};
    int CountNULL, num_1024;
    if (FullLog) REPORT("\nNULL return & check errno:\n");
    UINT Size;
    Limit limit_total(TOTAL_MB_ALLOC), no_limit(0);
    void **buf_1024 = (void**)Tmalloc(MAXNUM_1024*sizeof(void*));

    REQUIRE(buf_1024);
    /* We must have space for pointers when memory limit is hit.
       Reserve enough for the worst case, taking into account race for
       limited space between threads.
    */
    PointerList.reserve(TOTAL_MB_ALLOC*MByte/MinSize);

    /* There is a bug in the specific version of GLIBC (2.5-12) shipped
       with RHEL5 that leads to erroneous working of the test
       on Intel(R) 64 and Itanium(R) architecture when setrlimit-related part is enabled.
       Switching to GLIBC 2.5-18 from RHEL5.1 resolved the issue.
     */
    if (perProcessLimits)
        limitBarrier->wait(limit_total);
    else
        limitMem(MB_PER_THREAD);

    /* regression test against the bug in allocator when it dereference nullptr
       while lack of memory
    */
    for (num_1024=0; num_1024<MAXNUM_1024; num_1024++) {
        buf_1024[num_1024] = Tcalloc(1024, 1);
        if (! buf_1024[num_1024]) {
            ASSERT_ERRNO(errno == ENOMEM, nullptr);
            break;
        }
    }
    for (int i=0; i<num_1024; i++)
        Tfree(buf_1024[i]);
    Tfree(buf_1024);

    do {
        Size=rand()%(MaxSize-MinSize)+MinSize;
        tmp=Tmalloc(Size);
        if (tmp != nullptr)
        {
            myMemset(tmp, 0, Size);
            PointerList.push_back(MemStruct(tmp, Size));
        }
    } while(tmp != nullptr);
    ASSERT_ERRNO(errno == ENOMEM, nullptr);
    if (FullLog) REPORT("\n");

    // preparation complete, now running tests
    // malloc
    if (FullLog) REPORT("malloc....");
    CountNULL = 0;
    while (CountNULL==0)
        for (int j=0; j<COUNT_TESTS; j++)
        {
            Size=rand()%(MaxSize-MinSize)+MinSize;
            errno = ENOMEM+j+1;
            tmp=Tmalloc(Size);
            if (tmp == nullptr)
            {
                CountNULL++;
                if ( CHECK_ERRNO(errno != ENOMEM) ) {
                    CountErrors++;
                    if (ShouldReportError()) REPORT("nullptr returned, error: errno (%d) != ENOMEM\n", errno);
                }
            }
            else
            {
                // Technically, if malloc returns a non-null pointer, it is allowed to set errno anyway.
                // However, on most systems it does not set errno.
                bool known_issue = false;
#if __unix__ || __ANDROID__
                if( CHECK_ERRNO(errno==ENOMEM) ) known_issue = true;
#endif /* __unix__ */
                if ( CHECK_ERRNO(errno != ENOMEM+j+1) && !known_issue) {
                    CountErrors++;
                    if (ShouldReportError()) REPORT("error: errno changed to %d though valid pointer was returned\n", errno);
                }
                myMemset(tmp, 0, Size);
                PointerList.push_back(MemStruct(tmp, Size));
            }
        }
    if (FullLog) REPORT("end malloc\n");
    if (CountErrors) REPORT("%s\n",strError);
    else if (FullLog) REPORT("%s\n",strOk);
    if (CountErrors) error_occurred = true;

    //calloc
    if (FullLog) REPORT("calloc....");
    CountNULL = 0;
    while (CountNULL==0)
        for (int j=0; j<COUNT_TESTS; j++)
        {
            Size=rand()%(MaxSize-MinSize)+MinSize;
            errno = ENOMEM+j+1;
            tmp=Tcalloc(COUNT_ELEM_CALLOC,Size);
            if (tmp == nullptr)
            {
                CountNULL++;
                if ( CHECK_ERRNO(errno != ENOMEM) ){
                    CountErrors++;
                    if (ShouldReportError()) REPORT("nullptr returned, error: errno(%d) != ENOMEM\n", errno);
                }
            }
            else
            {
                // Technically, if calloc returns a non-null pointer, it is allowed to set errno anyway.
                // However, on most systems it does not set errno.
                bool known_issue = false;
#if __unix__
                if( CHECK_ERRNO(errno==ENOMEM) ) known_issue = true;
#endif /* __unix__ */
                if ( CHECK_ERRNO(errno != ENOMEM+j+1) && !known_issue ) {
                    CountErrors++;
                    if (ShouldReportError()) REPORT("error: errno changed to %d though valid pointer was returned\n", errno);
                }
                PointerList.push_back(MemStruct(tmp, Size));
            }
        }
    if (FullLog) REPORT("end calloc\n");
    if (CountErrors) REPORT("%s\n",strError);
    else if (FullLog) REPORT("%s\n",strOk);
    if (CountErrors) error_occurred = true;
    if (FullLog) REPORT("realloc....");
    CountNULL = 0;
    if (PointerList.size() > 0)
        while (CountNULL==0)
            for (size_t i=0; i<(size_t)COUNT_TESTS && i<PointerList.size(); i++)
            {
                errno = 0;
                tmp=Trealloc(PointerList[i].Pointer,PointerList[i].Size*2);
                if (tmp != nullptr) // same or another place
                {
                    bool known_issue = false;
#if __unix__
                    if( errno==ENOMEM ) known_issue = true;
#endif /* __unix__ */
                    if (errno != 0 && !known_issue) {
                        CountErrors++;
                        if (ShouldReportError()) REPORT("valid pointer returned, error: errno not kept\n");
                    }
                    // newly allocated area have to be zeroed
                    myMemset((char*)tmp + PointerList[i].Size, 0, PointerList[i].Size);
                    PointerList[i].Pointer = tmp;
                    PointerList[i].Size *= 2;
                } else {
                    CountNULL++;
                    if ( CHECK_ERRNO(errno != ENOMEM) )
                    {
                        CountErrors++;
                        if (ShouldReportError()) REPORT("nullptr returned, error: errno(%d) != ENOMEM\n", errno);
                    }
                    // check data integrity
                    if (NonZero(PointerList[i].Pointer, PointerList[i].Size)) {
                        CountErrors++;
                        if (ShouldReportError()) REPORT("nullptr returned, error: data changed\n");
                    }
                }
            }
    if (FullLog) REPORT("realloc end\n");
    if (CountErrors) REPORT("%s\n",strError);
    else if (FullLog) REPORT("%s\n",strOk);
    if (CountErrors) error_occurred = true;
    for (UINT i=0; i<PointerList.size(); i++)
    {
        Tfree(PointerList[i].Pointer);
    }

    if (perProcessLimits)
        limitBarrier->wait(no_limit);
    else
        limitMem(0);
}
#endif /* #if !__APPLE__ */

void CMemTest::UniquePointer()
{
    static std::atomic<int> CountErrors{0};
    int **MasPointer = (int **)Tmalloc(sizeof(int*)*COUNT_ELEM);
    size_t *MasCountElem = (size_t*)Tmalloc(sizeof(size_t)*COUNT_ELEM);
    if (FullLog) REPORT("\nUnique pointer using 0\n");
    REQUIRE((MasCountElem && MasPointer));
    //
    //-------------------------------------------------------
    //malloc
    for (int i=0; i<COUNT_ELEM; i++)
    {
        MasCountElem[i]=rand()%MAX_SIZE;
        MasPointer[i]=(int*)Tmalloc(MasCountElem[i]*sizeof(int));
        if (nullptr == MasPointer[i])
            MasCountElem[i]=0;
        memset(MasPointer[i], 0, sizeof(int)*MasCountElem[i]);
    }
    if (FullLog) REPORT("malloc....");
    for (UINT i=0; i<COUNT_ELEM-1; i++)
    {
        if (size_t badOff = NonZero(MasPointer[i], sizeof(int)*MasCountElem[i])) {
            CountErrors++;
            if (ShouldReportError())
                REPORT("error, detect non-zero at %p\n", (char*)MasPointer[i]+badOff-1);
        }
        memset(MasPointer[i], 1, sizeof(int)*MasCountElem[i]);
    }
    if (CountErrors) REPORT("%s\n",strError);
    else if (FullLog) REPORT("%s\n",strOk);
    if (CountErrors) error_occurred = true;
    //----------------------------------------------------------
    //calloc
    for (int i=0; i<COUNT_ELEM; i++)
        Tfree(MasPointer[i]);
    for (long i=0; i<COUNT_ELEM; i++)
    {
        MasPointer[i]=(int*)Tcalloc(MasCountElem[i]*sizeof(int),2);
        if (nullptr == MasPointer[i])
            MasCountElem[i]=0;
    }
    if (FullLog) REPORT("calloc....");
    for (int i=0; i<COUNT_ELEM-1; i++)
    {
        if (size_t badOff = NonZero(MasPointer[i], sizeof(int)*MasCountElem[i])) {
            CountErrors++;
            if (ShouldReportError())
                REPORT("error, detect non-zero at %p\n", (char*)MasPointer[i]+badOff-1);
        }
        memset(MasPointer[i], 1, sizeof(int)*MasCountElem[i]);
    }
    if (CountErrors) REPORT("%s\n",strError);
    else if (FullLog) REPORT("%s\n",strOk);
    if (CountErrors) error_occurred = true;
    //---------------------------------------------------------
    //realloc
    for (int i=0; i<COUNT_ELEM; i++)
    {
        MasCountElem[i]*=2;
        *(MasPointer+i)=
            (int*)Trealloc(*(MasPointer+i),MasCountElem[i]*sizeof(int));
        if (nullptr == MasPointer[i])
            MasCountElem[i]=0;
        memset(MasPointer[i], 0, sizeof(int)*MasCountElem[i]);
    }
    if (FullLog) REPORT("realloc....");
    for (int i=0; i<COUNT_ELEM-1; i++)
    {
        if (NonZero(MasPointer[i], sizeof(int)*MasCountElem[i]))
            CountErrors++;
        memset(MasPointer[i], 1, sizeof(int)*MasCountElem[i]);
    }
    if (CountErrors) REPORT("%s\n",strError);
    else if (FullLog) REPORT("%s\n",strOk);
    if (CountErrors) error_occurred = true;
    for (int i=0; i<COUNT_ELEM; i++)
        Tfree(MasPointer[i]);
    Tfree(MasCountElem);
    Tfree(MasPointer);
}

bool CMemTest::ShouldReportError()
{
    if (FullLog)
        return true;
    else
        if (firstTime) {
            firstTime = false;
            return true;
        } else
            return false;
}

void CMemTest::Free_NULL()
{
    static std::atomic<int> CountErrors{0};
    if (FullLog) REPORT("\ncall free with parameter nullptr....");
    errno = 0;
    for (int i=0; i<COUNTEXPERIMENT; i++)
    {
        Tfree(nullptr);
        if (CHECK_ERRNO(errno))
        {
            CountErrors++;
            if (ShouldReportError()) REPORT("error is found by a call free with parameter nullptr\n");
        }
    }
    if (CountErrors) REPORT("%s\n",strError);
    else if (FullLog) REPORT("%s\n",strOk);
    if (CountErrors) error_occurred = true;
}

void CMemTest::TestAlignedParameters()
{
    void *memptr;
    int ret;

    if (Rposix_memalign) {
        // alignment isn't power of 2
        for (int bad_align=3; bad_align<16; bad_align++)
            if (bad_align&(bad_align-1)) {
                ret = Tposix_memalign(nullptr, bad_align, 100);
                REQUIRE(EINVAL==ret);
            }

        memptr = &ret;
        ret = Tposix_memalign(&memptr, 5*sizeof(void*), 100);
        REQUIRE_MESSAGE(memptr == &ret,
               "memptr should not be changed after unsuccessful call");
        REQUIRE(EINVAL==ret);

        // alignment is power of 2, but not a multiple of sizeof(void *),
        // we expect that sizeof(void*) > 2
        ret = Tposix_memalign(nullptr, 2, 100);
        REQUIRE(EINVAL==ret);
    }
    if (Raligned_malloc) {
        // alignment isn't power of 2
        for (int bad_align=3; bad_align<16; bad_align++)
            if (bad_align&(bad_align-1)) {
                memptr = Taligned_malloc(100, bad_align);
                REQUIRE(memptr == nullptr);
                ASSERT_ERRNO(EINVAL==errno, nullptr);
            }

        // size is zero
        memptr = Taligned_malloc(0, 16);
        REQUIRE_MESSAGE(memptr == nullptr, "size is zero, so must return nullptr");
        ASSERT_ERRNO(EINVAL==errno, nullptr);
    }
    if (Taligned_free) {
        // nullptr pointer is OK to free
        errno = 0;
        Taligned_free(nullptr);
        /* As there is no return value for free, strictly speaking we can't
           check errno here. But checked implementations obey the assertion.
        */
        ASSERT_ERRNO(0==errno, nullptr);
    }
    if (Raligned_realloc) {
        for (int i=1; i<20; i++) {
            // checks that calls work correctly in presence of non-zero errno
            errno = i;
            void *ptr = Taligned_malloc(i*10, 128);
            REQUIRE(ptr != nullptr);
            ASSERT_ERRNO(0!=errno, nullptr);
            // if size is zero and pointer is not nullptr, works like free
            memptr = Taligned_realloc(ptr, 0, 64);
            REQUIRE(memptr == nullptr);
            ASSERT_ERRNO(0!=errno, nullptr);
        }
        // alignment isn't power of 2
        for (int bad_align=3; bad_align<16; bad_align++)
            if (bad_align&(bad_align-1)) {
                void *ptr = &bad_align;
                memptr = Taligned_realloc(&ptr, 100, bad_align);
                REQUIRE(memptr == nullptr);
                REQUIRE(&bad_align==ptr);
                ASSERT_ERRNO(EINVAL==errno, nullptr);
            }
    }
}

void CMemTest::RunAllTests(int total_threads)
{
    Zerofilling();
    Free_NULL();
    InvariantDataRealloc(/*aligned=*/false, 8*MByte, /*checkData=*/true);
    if (Raligned_realloc)
        InvariantDataRealloc(/*aligned=*/true, 8*MByte, /*checkData=*/true);
    TestAlignedParameters();
    UniquePointer();
    AddrArifm();
#if __APPLE__ || __TBB_USE_THREAD_SANITIZER
    REPORT("Known issue: some tests are skipped on macOS\n");
#else
    // TODO: enable
    NULLReturn(1*MByte,100*MByte,total_threads);
#endif
    if (FullLog) REPORT("Tests for %d threads ended\n", total_threads);
}

// TODO: fix the tests to support UWP mode
#if !__TBB_WIN8UI_SUPPORT

TEST_CASE("MAIN TEST") {
    Rmalloc=scalable_malloc;
    Rrealloc=scalable_realloc;
    Rcalloc=scalable_calloc;
    Tfree=scalable_free;
    Rposix_memalign=scalable_posix_memalign;
    Raligned_malloc=scalable_aligned_malloc;
    Raligned_realloc=scalable_aligned_realloc;
    Taligned_free=scalable_aligned_free;

    // Check if we were called to test standard behavior
    // TODO: enable this mode
    // setSystemAllocs();
#if __unix__
    /* According to man pthreads
       "NPTL threads do not share resource limits (fixed in kernel 2.6.10)".
       Use per-threads limits for affected systems.
     */
    if ( utils::LinuxKernelVersion() < 2*1000000 + 6*1000 + 10)
        perProcessLimits = false;
#endif
    //-------------------------------------
#if __APPLE__ || __TBB_USE_SANITIZERS
    /* Skip due to lack of memory limit enforcing under macOS. */
    //Skip this test under ASAN , as OOM condition breaks the ASAN as well
#else
    limitMem(200);
    ReallocParam();
    limitMem(0);
#endif

//for linux and dynamic runtime errno is used to check allocator functions
//check if library compiled with /MD(d) and we can use errno
#if _MSC_VER
#if defined(_MT) && defined(_DLL) //check errno if test itself compiled with /MD(d) only
    char*  version_info_block = nullptr;
    int version_info_block_size;
    LPVOID comments_block = nullptr;
    UINT comments_block_size;
#ifdef _DEBUG
#define __TBBMALLOCDLL "tbbmalloc_debug.dll"
#else  //_DEBUG
#define __TBBMALLOCDLL "tbbmalloc.dll"
#endif //_DEBUG
    version_info_block_size = GetFileVersionInfoSize( __TBBMALLOCDLL, (LPDWORD)&version_info_block_size );
    if( version_info_block_size
        && ((version_info_block = (char*)malloc(version_info_block_size)) != nullptr)
        && GetFileVersionInfo(  __TBBMALLOCDLL, NULL, version_info_block_size, version_info_block )
        && VerQueryValue( version_info_block, "\\StringFileInfo\\000004b0\\Comments", &comments_block, &comments_block_size )
        && strstr( (char*)comments_block, "/MD" )
        ){
            __tbb_test_errno = true;
     }
     if( version_info_block ) free( version_info_block );
#endif // defined(_MT) && defined(_DLL)
#else  // _MSC_VER
    __tbb_test_errno = true;
#endif // _MSC_VER

    CheckArgumentsOverflow();
    CheckReallocLeak();
    for( int p=MaxThread; p>=MinThread; --p ) {
        for (int limit=0; limit<2; limit++) {
            int ret = scalable_allocation_mode(TBBMALLOC_SET_SOFT_HEAP_LIMIT, 16*1024*limit);
            REQUIRE(ret==TBBMALLOC_OK);
            utils::SpinBarrier *barrier = new utils::SpinBarrier(p);
            utils::NativeParallelFor( p, RoundRobin(p, barrier, Verbose) );
            delete barrier;
        }
    }
    int ret = scalable_allocation_mode(TBBMALLOC_SET_SOFT_HEAP_LIMIT, 0);
    REQUIRE(ret==TBBMALLOC_OK);
    REQUIRE(!error_occurred);
}

#endif /* __TBB_WIN8UI_SUPPORT	 */
