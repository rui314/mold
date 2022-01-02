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

#ifdef __cplusplus
#error For testing purpose, this file should be compiled with a C compiler, not C++
#endif /*__cplusplus */

#include "tbb/scalable_allocator.h"
#include <stdio.h>
#include <assert.h>
#include <stdlib.h> /* for atexit */

/*
 *  The test is to check if the scalable_allocator.h and its functions
 *  can be used from pure C programs; also some regression checks are done
 */

#if __unix__
/* huge pages supported only under Linux so far */
const int ExpectedResultHugePages = TBBMALLOC_OK;
#else
const int ExpectedResultHugePages = TBBMALLOC_NO_EFFECT;
#endif

/* bool type definition for C */
#if (defined(_MSC_VER) && _MSC_VER < 1800) || __sun || __SUNPRO_CC
typedef int bool;
#define false 0
#define true 1
#else
#include <stdbool.h>
#endif

/* test that it's possible to call allocation function from atexit
   after mallocProcessShutdownNotification() called */
static void MyExit(void) {
    void *p = scalable_malloc(32);
    assert(p);
    scalable_free(p);
}

int main(void) {
    size_t i, j;
    int curr_mode, res;
    void *p1, *p2;

    atexit( MyExit );
    for ( curr_mode = 0; curr_mode<=1; curr_mode++) {
        assert(ExpectedResultHugePages ==
               scalable_allocation_mode(TBBMALLOC_USE_HUGE_PAGES, !curr_mode));
        p1 = scalable_malloc(10*1024*1024);
        assert(p1);
        assert(ExpectedResultHugePages ==
               scalable_allocation_mode(TBBMALLOC_USE_HUGE_PAGES, curr_mode));
        scalable_free(p1);
    }

    for( i=0; i<=1<<16; ++i) {
        p1 = scalable_malloc(i);
        if( !p1 )
            printf("Warning: there should be memory but scalable_malloc returned NULL\n");
        scalable_free(p1);
    }
    p1 = p2 = NULL;
    for( i=1024*1024; ; i/=2 )
    {
        scalable_free(p1);
        p1 = scalable_realloc(p2, i);
        p2 = scalable_calloc(i, 32);
        if (p2) {
            if (i<sizeof(size_t)) {
                for (j=0; j<i; j++)
                    assert(0==*((char*)p2+j));
            } else {
                for (j=0; j<i; j+=sizeof(size_t))
                    assert(0==*((size_t*)p2+j));
            }
        }
        scalable_free(p2);
        p2 = scalable_malloc(i);
        if (i==0) break;
    }
    for( i=1; i<1024*1024; i*=2 )
    {
        scalable_free(p1);
        p1 = scalable_realloc(p2, i);
        p2 = scalable_malloc(i);
    }
    scalable_free(p1);
    scalable_free(p2);
    res = scalable_allocation_command(TBBMALLOC_CLEAN_ALL_BUFFERS, NULL);
    assert(res == TBBMALLOC_OK);
    res = scalable_allocation_command(TBBMALLOC_CLEAN_THREAD_BUFFERS, NULL);
    /* expect all caches cleaned before, so got nothing from CLEAN_THREAD_BUFFERS */
    assert(res == TBBMALLOC_NO_EFFECT);
    /* check that invalid param argument give expected result*/
    res = scalable_allocation_command(TBBMALLOC_CLEAN_THREAD_BUFFERS,
                                      (void*)(intptr_t)1);
    assert(res == TBBMALLOC_INVALID_PARAM);
    printf("done\n");
    return 0;
}
