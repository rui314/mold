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

#ifndef _TBB_co_context_H
#define _TBB_co_context_H

#include "oneapi/tbb/detail/_config.h"

#if __TBB_RESUMABLE_TASKS

#include <cstddef>
#include <cstdint>

#if _WIN32 || _WIN64
#include <windows.h>
#else
// ucontext.h API is deprecated since macOS 10.6
#if __APPLE__
    #if __INTEL_COMPILER
        #pragma warning(push)
        #pragma warning(disable:1478)
    #elif __clang__
        #pragma clang diagnostic push
        #pragma clang diagnostic ignored "-Wdeprecated-declarations"
    #endif
#endif // __APPLE__

#include <ucontext.h>
#include <sys/mman.h> // mprotect

#include "governor.h" // default_page_size()

#ifndef MAP_STACK
// macOS* does not define MAP_STACK
#define MAP_STACK 0
#endif
#ifndef MAP_ANONYMOUS
// macOS* defines MAP_ANON, which is deprecated in Linux*.
#define MAP_ANONYMOUS MAP_ANON
#endif
#endif // _WIN32 || _WIN64

namespace tbb {
namespace detail {
namespace r1 {

#if _WIN32 || _WIN64
    typedef LPVOID coroutine_type;
#else
    struct coroutine_type {
        coroutine_type() : my_context(), my_stack(), my_stack_size() {}
        ucontext_t my_context;
        void* my_stack;
        std::size_t my_stack_size;
    };
#endif

    // Forward declaration of the coroutine API.
    void create_coroutine(coroutine_type& c, std::size_t stack_size, void* arg);
    void current_coroutine(coroutine_type& c);
    void swap_coroutine(coroutine_type& prev_coroutine, coroutine_type& new_coroutine);
    void destroy_coroutine(coroutine_type& c);

class co_context {
    enum co_state {
        co_invalid,
        co_suspended,
        co_executing,
        co_destroyed
    };
    coroutine_type      my_coroutine;
    co_state            my_state;

public:
    co_context(std::size_t stack_size, void* arg)
        : my_state(stack_size ? co_suspended : co_executing)
    {
        if (stack_size) {
            __TBB_ASSERT(arg != 0, nullptr);
            create_coroutine(my_coroutine, stack_size, arg);
        } else {
            current_coroutine(my_coroutine);
        }
    }

    ~co_context() {
        __TBB_ASSERT(1 << my_state & (1 << co_suspended | 1 << co_executing), NULL);
        if (my_state == co_suspended)
            destroy_coroutine(my_coroutine);
        my_state = co_destroyed;
    }

    void resume(co_context& target) {
        // Do not create non-trivial objects on the stack of this function. They might never be destroyed.
        __TBB_ASSERT(my_state == co_executing, NULL);
        __TBB_ASSERT(target.my_state == co_suspended, NULL);

        my_state = co_suspended;
        target.my_state = co_executing;

        // 'target' can reference an invalid object after swap_coroutine. Do not access it.
        swap_coroutine(my_coroutine, target.my_coroutine);

        __TBB_ASSERT(my_state == co_executing, NULL);
    }
};

#if _WIN32 || _WIN64
/* [[noreturn]] */ void __stdcall co_local_wait_for_all(void* arg) noexcept;
#else
/* [[noreturn]] */ void co_local_wait_for_all(unsigned hi, unsigned lo) noexcept;
#endif

#if _WIN32 || _WIN64
inline void create_coroutine(coroutine_type& c, std::size_t stack_size, void* arg) {
    __TBB_ASSERT(arg, NULL);
    c = CreateFiber(stack_size, co_local_wait_for_all, arg);
    __TBB_ASSERT(c, NULL);
}

inline void current_coroutine(coroutine_type& c) {
    c = IsThreadAFiber() ? GetCurrentFiber() :
        ConvertThreadToFiberEx(nullptr, FIBER_FLAG_FLOAT_SWITCH);
    __TBB_ASSERT(c, NULL);
}

inline void swap_coroutine(coroutine_type& prev_coroutine, coroutine_type& new_coroutine) {
    if (!IsThreadAFiber()) {
        ConvertThreadToFiberEx(nullptr, FIBER_FLAG_FLOAT_SWITCH);
    }
    __TBB_ASSERT(new_coroutine, NULL);
    prev_coroutine = GetCurrentFiber();
    __TBB_ASSERT(prev_coroutine, NULL);
    SwitchToFiber(new_coroutine);
}

inline void destroy_coroutine(coroutine_type& c) {
    __TBB_ASSERT(c, NULL);
    DeleteFiber(c);
}
#else // !(_WIN32 || _WIN64)

inline void create_coroutine(coroutine_type& c, std::size_t stack_size, void* arg) {
    const std::size_t REG_PAGE_SIZE = governor::default_page_size();
    const std::size_t page_aligned_stack_size = (stack_size + (REG_PAGE_SIZE - 1)) & ~(REG_PAGE_SIZE - 1);
    const std::size_t protected_stack_size = page_aligned_stack_size + 2 * REG_PAGE_SIZE;

    // Allocate the stack with protection property
    std::uintptr_t stack_ptr = (std::uintptr_t)mmap(NULL, protected_stack_size, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_STACK, -1, 0);
    __TBB_ASSERT((void*)stack_ptr != MAP_FAILED, NULL);

    // Allow read write on our stack (guarded pages are still protected)
    int err = mprotect((void*)(stack_ptr + REG_PAGE_SIZE), page_aligned_stack_size, PROT_READ | PROT_WRITE);
    __TBB_ASSERT_EX(!err, NULL);

    // Remember the stack state
    c.my_stack = (void*)(stack_ptr + REG_PAGE_SIZE);
    c.my_stack_size = page_aligned_stack_size;

    err = getcontext(&c.my_context);
    __TBB_ASSERT_EX(!err, NULL);

    c.my_context.uc_link = 0;
    // cast to char* to disable FreeBSD clang-3.4.1 'incompatible type' error
    c.my_context.uc_stack.ss_sp = (char*)c.my_stack;
    c.my_context.uc_stack.ss_size = c.my_stack_size;
    c.my_context.uc_stack.ss_flags = 0;

    typedef void(*coroutine_func_t)();

    std::uintptr_t addr = std::uintptr_t(arg);
    unsigned lo = unsigned(addr);
    unsigned hi = unsigned(std::uint64_t(addr) >> 32);
    __TBB_ASSERT(sizeof(addr) == 8 || hi == 0, nullptr);

    makecontext(&c.my_context, (coroutine_func_t)co_local_wait_for_all, 2, hi, lo);
}

inline void current_coroutine(coroutine_type& c) {
    int err = getcontext(&c.my_context);
    __TBB_ASSERT_EX(!err, NULL);
}

inline void swap_coroutine(coroutine_type& prev_coroutine, coroutine_type& new_coroutine) {
    int err = swapcontext(&prev_coroutine.my_context, &new_coroutine.my_context);
    __TBB_ASSERT_EX(!err, NULL);
}

inline void destroy_coroutine(coroutine_type& c) {
    const std::size_t REG_PAGE_SIZE = governor::default_page_size();
    // Free stack memory with guarded pages
    munmap((void*)((std::uintptr_t)c.my_stack - REG_PAGE_SIZE), c.my_stack_size + 2 * REG_PAGE_SIZE);
    // Clear the stack state afterwards
    c.my_stack = NULL;
    c.my_stack_size = 0;
}

#if __APPLE__
    #if __INTEL_COMPILER
        #pragma warning(pop) // 1478 warning
    #elif __clang__
        #pragma clang diagnostic pop // "-Wdeprecated-declarations"
    #endif
#endif

#endif // _WIN32 || _WIN64

} // namespace r1
} // namespace detail
} // namespace tbb

#endif /* __TBB_RESUMABLE_TASKS */

#endif /* _TBB_co_context_H */

