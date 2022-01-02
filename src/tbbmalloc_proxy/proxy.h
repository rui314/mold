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

#ifndef _TBB_malloc_proxy_H_
#define _TBB_malloc_proxy_H_

#define MALLOC_UNIXLIKE_OVERLOAD_ENABLED __linux__
#define MALLOC_ZONE_OVERLOAD_ENABLED __APPLE__

// MALLOC_UNIXLIKE_OVERLOAD_ENABLED depends on MALLOC_CHECK_RECURSION stuff
// TODO: limit MALLOC_CHECK_RECURSION to *_OVERLOAD_ENABLED only
#if __unix__ || __APPLE__ || MALLOC_UNIXLIKE_OVERLOAD_ENABLED
#define MALLOC_CHECK_RECURSION 1
#endif

#include "oneapi/tbb/detail/_config.h"
#include <stddef.h>

extern "C" {
    TBBMALLOC_EXPORT void   __TBB_malloc_safer_free( void *ptr, void (*original_free)(void*));
    TBBMALLOC_EXPORT void * __TBB_malloc_safer_realloc( void *ptr, size_t, void* );
    TBBMALLOC_EXPORT void * __TBB_malloc_safer_aligned_realloc( void *ptr, size_t, size_t, void* );
    TBBMALLOC_EXPORT size_t __TBB_malloc_safer_msize( void *ptr, size_t (*orig_msize_crt80d)(void*));
    TBBMALLOC_EXPORT size_t __TBB_malloc_safer_aligned_msize( void *ptr, size_t, size_t, size_t (*orig_msize_crt80d)(void*,size_t,size_t));

#if MALLOC_ZONE_OVERLOAD_ENABLED
    void   __TBB_malloc_free_definite_size(void *object, size_t size);
#endif
} // extern "C"

// Struct with original free() and _msize() pointers
struct orig_ptrs {
    void   (*free) (void*);
    size_t (*msize)(void*);
};

struct orig_aligned_ptrs {
    void   (*aligned_free) (void*);
    size_t (*aligned_msize)(void*,size_t,size_t);
};

#endif /* _TBB_malloc_proxy_H_ */
