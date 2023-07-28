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

#include "common/test.h"

#if !__TBB_WIN8UI_SUPPORT

#include <stdio.h>
#include "tbb/parallel_for.h"
#include "tbb/global_control.h"
#include "tbb/enumerable_thread_specific.h"

#include "common/config.h"
#include "common/utils.h"
#include "common/utils_concurrency_limit.h"
#include "common/utils_report.h"
#include "common/vector_types.h"
#include "common/cpu_usertime.h"
#include "common/spin_barrier.h"
#include "common/exception_handling.h"
#include "common/concepts_common.h"
#include "test_partitioner.h"

#include <cstddef>
#include <vector>

//! \file test_numa_dist.cpp
//! \brief Test for [internal] functionality
#if _MSC_VER
#pragma warning (push)
// Suppress conditional expression is constant
#pragma warning (disable: 4127)
#if __TBB_MSVC_UNREACHABLE_CODE_IGNORED
    // Suppress pointless "unreachable code" warning.
    #pragma warning (disable: 4702)
#endif
#if defined(_Wp64)
    // Workaround for overzealous compiler warnings in /Wp64 mode
    #pragma warning (disable: 4267)
#endif
#define _SCL_SECURE_NO_WARNINGS
#endif //#if _MSC_VER


struct numa {
    WORD processorGroupCount;
    std::vector<DWORD> numaProcessors;
    DWORD maxProcessors;
    numa() : processorGroupCount(GetMaximumProcessorGroupCount()), maxProcessors(GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)){   
        numaProcessors.resize(processorGroupCount);
       for (WORD i = 0; i < processorGroupCount; i++) {
            this->numaProcessors[i] = GetActiveProcessorCount((i));
        }        
    }
};


int TestNumaDistribution(std::vector<DWORD> &validateProcgrp, int additionalParallelism, bool allThreads){
    validateProcgrp.resize(GetMaximumProcessorGroupCount());
    PROCESSOR_NUMBER proc;
    struct numa nodes;
    GetThreadIdealProcessorEx(GetCurrentThread(), &proc);
    int master_thread_proc_grp = proc.Group;
    int requested_parallelism;
    if (allThreads) 
        requested_parallelism = additionalParallelism;
    else 
        requested_parallelism = nodes.numaProcessors.at(master_thread_proc_grp) + additionalParallelism;
    tbb::global_control global_limit(oneapi::tbb::global_control::max_allowed_parallelism, 1024);
    tbb::enumerable_thread_specific< std::pair<int, int> > tls;
    tbb::enumerable_thread_specific< double > tls_dummy;
    tbb::static_partitioner s;
  
    utils::SpinBarrier sb(requested_parallelism);
    oneapi::tbb::task_arena limited(requested_parallelism);
    limited.execute([&]() {

        tbb::parallel_for(0, requested_parallelism, [&](int)
            {                    
                PROCESSOR_NUMBER proc;
                if (GetThreadIdealProcessorEx(GetCurrentThread(), &proc))
                {
                    tls.local() = std::pair<int, int>(proc.Group, proc.Number);
                    sb.wait();
                }
            }, s);
        for (const auto& it : tls) {
           validateProcgrp[it.first]++;
        }
      });
    

    return master_thread_proc_grp;
}

//! Testing Numa Thread Distribution Stability
//! \brief \ref stress
TEST_CASE("Numa stability for the same node") {
    numa example;
    std::vector<DWORD> validateProcgrp;
    
    int numaGrp = TestNumaDistribution(validateProcgrp,0, 0);
    std::vector<DWORD> result(GetMaximumProcessorGroupCount(), 0);
    result[numaGrp] = example.numaProcessors[numaGrp];
    REQUIRE(validateProcgrp == result);
}

//! Testing Numa Thread Distribution Overflow
//! \brief \ref stress
TEST_CASE("Numa overflow") {
    numa example;
    std::vector<DWORD> validateProcgrp;
    
    int numaGrp = TestNumaDistribution(validateProcgrp, 1, 0);
    std::vector<DWORD> result(GetMaximumProcessorGroupCount(), 0);
    if (example.processorGroupCount <= 1) { // for single Numa node
       result[numaGrp] = example.numaProcessors[numaGrp] + 1;
    } else {
       result[numaGrp] = example.numaProcessors[numaGrp];
       result[(numaGrp+1)% GetMaximumProcessorGroupCount()] = 1;
    }
    REQUIRE(validateProcgrp == result);
}

//! Testing Numa Thread Distribution Maximum
//! \brief \ref stress
TEST_CASE("Numa all threads") {
    numa example;
    std::vector<DWORD> validateProcgrp;
    TestNumaDistribution(validateProcgrp, example.maxProcessors, 1);
    REQUIRE(validateProcgrp == example.numaProcessors);
}

//! Testing Numa Thread Distribution Doubled Max
//! \brief \ref stress
TEST_CASE("Double threads") {
    numa example;
    std::vector<DWORD> validateProcgrp;
    std::vector<DWORD> result(example.numaProcessors.size(), 0);
    for (size_t i = 0; i < example.numaProcessors.size(); i++) result[i] = 2 * example.numaProcessors[i];
    TestNumaDistribution(validateProcgrp, example.maxProcessors * 2, 1);
    REQUIRE(validateProcgrp == result);
}

#if _MSC_VER
#pragma warning (pop)
#endif

#endif // !__TBB_WIN8UI_SUPPORT
