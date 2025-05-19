/*
    Copyright (c) 2020-2024 Intel Corporation

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

#ifndef __TBB__task_H
#define __TBB__task_H

#include "_config.h"
#include "_assert.h"
#include "_template_helpers.h"
#include "_small_object_pool.h"

#include "../profiling.h"

#include <cstddef>
#include <cstdint>
#include <climits>
#include <utility>
#include <atomic>
#include <mutex>

namespace tbb {
namespace detail {

namespace d1 {
using slot_id = unsigned short;
constexpr slot_id no_slot = slot_id(~0);
constexpr slot_id any_slot = slot_id(~1);

class task;
class wait_context;
class task_group_context;
struct execution_data;
class wait_tree_vertex_interface;
class task_arena_base;
}

namespace d2 {
class task_group;
class task_group_base;
}

namespace r1 {
//! Task spawn/wait entry points
TBB_EXPORT void __TBB_EXPORTED_FUNC spawn(d1::task& t, d1::task_group_context& ctx);
TBB_EXPORT void __TBB_EXPORTED_FUNC spawn(d1::task& t, d1::task_group_context& ctx, d1::slot_id id);
TBB_EXPORT void __TBB_EXPORTED_FUNC execute_and_wait(d1::task& t, d1::task_group_context& t_ctx, d1::wait_context&, d1::task_group_context& w_ctx);
TBB_EXPORT void __TBB_EXPORTED_FUNC wait(d1::wait_context&, d1::task_group_context& ctx);
TBB_EXPORT d1::slot_id __TBB_EXPORTED_FUNC execution_slot(const d1::execution_data*);
TBB_EXPORT d1::slot_id __TBB_EXPORTED_FUNC execution_slot(const d1::task_arena_base&);
TBB_EXPORT d1::task_group_context* __TBB_EXPORTED_FUNC current_context();
TBB_EXPORT d1::wait_tree_vertex_interface* get_thread_reference_vertex(d1::wait_tree_vertex_interface* wc);

// Do not place under __TBB_RESUMABLE_TASKS. It is a stub for unsupported platforms.
struct suspend_point_type;
using suspend_callback_type = void(*)(void*, suspend_point_type*);
//! The resumable tasks entry points
TBB_EXPORT void __TBB_EXPORTED_FUNC suspend(suspend_callback_type suspend_callback, void* user_callback);
TBB_EXPORT void __TBB_EXPORTED_FUNC resume(suspend_point_type* tag);
TBB_EXPORT suspend_point_type* __TBB_EXPORTED_FUNC current_suspend_point();
TBB_EXPORT void __TBB_EXPORTED_FUNC notify_waiters(std::uintptr_t wait_ctx_addr);

class thread_data;
class task_dispatcher;
class external_waiter;
struct task_accessor;
struct task_arena_impl;
} // namespace r1

namespace d1 {

class task_arena;
using suspend_point = r1::suspend_point_type*;

#if __TBB_RESUMABLE_TASKS
template <typename F>
static void suspend_callback(void* user_callback, suspend_point sp) {
    // Copy user function to a new stack after the context switch to avoid a race when the previous
    // suspend point is resumed while the user_callback is being called.
    F user_callback_copy = *static_cast<F*>(user_callback);
    user_callback_copy(sp);
}

template <typename F>
void suspend(F f) {
    r1::suspend(&suspend_callback<F>, &f);
}

inline void resume(suspend_point tag) {
    r1::resume(tag);
}
#endif /* __TBB_RESUMABLE_TASKS */

// TODO align wait_context on cache lane
class wait_context {
    static constexpr std::uint64_t overflow_mask = ~((1LLU << 32) - 1);

    std::uint64_t m_version_and_traits{1};
    std::atomic<std::uint64_t> m_ref_count{};

    void add_reference(std::int64_t delta) {
        call_itt_task_notify(releasing, this);
        std::uint64_t r = m_ref_count.fetch_add(static_cast<std::uint64_t>(delta)) + static_cast<std::uint64_t>(delta);

        __TBB_ASSERT_EX((r & overflow_mask) == 0, "Overflow is detected");

        if (!r) {
            // Some external waiters or coroutine waiters sleep in wait list
            // Should to notify them that work is done
            std::uintptr_t wait_ctx_addr = std::uintptr_t(this);
            r1::notify_waiters(wait_ctx_addr);
        }
    }

    bool continue_execution() const {
        std::uint64_t r = m_ref_count.load(std::memory_order_acquire);
        __TBB_ASSERT_EX((r & overflow_mask) == 0, "Overflow is detected");
        return r > 0;
    }

    friend class r1::thread_data;
    friend class r1::task_dispatcher;
    friend class r1::external_waiter;
    friend class wait_context_vertex;
    friend struct r1::task_arena_impl;
    friend struct r1::suspend_point_type;
public:
    // Despite the internal reference count is uin64_t we limit the user interface with uint32_t
    // to preserve a part of the internal reference count for special needs.
    wait_context(std::uint32_t ref_count) : m_ref_count{ref_count} { suppress_unused_warning(m_version_and_traits); }
    wait_context(const wait_context&) = delete;

    ~wait_context() {
        __TBB_ASSERT(!continue_execution(), nullptr);
    }

    void reserve(std::uint32_t delta = 1) {
        add_reference(delta);
    }

    void release(std::uint32_t delta = 1) {
        add_reference(-std::int64_t(delta));
    }
};

class wait_tree_vertex_interface {
public:
    virtual void reserve(std::uint32_t delta = 1) = 0;
    virtual void release(std::uint32_t delta = 1) = 0;

protected:
    virtual ~wait_tree_vertex_interface() = default;
};

class wait_context_vertex : public wait_tree_vertex_interface {
public:
    wait_context_vertex(std::uint32_t ref = 0) : m_wait(ref) {}

    void reserve(std::uint32_t delta = 1) override {
        m_wait.reserve(delta);
    }

    void release(std::uint32_t delta = 1) override {
        m_wait.release(delta);
    }

    wait_context& get_context() {
        return m_wait;
    }
private:
    friend class d2::task_group;
    friend class d2::task_group_base;

    bool continue_execution() const {
        return m_wait.continue_execution();
    }

    wait_context m_wait;
};

class reference_vertex : public wait_tree_vertex_interface {
public:
    reference_vertex(wait_tree_vertex_interface* parent, std::uint32_t ref_count) : my_parent{parent}, m_ref_count{ref_count}
    {}

    void reserve(std::uint32_t delta = 1) override {
        if (m_ref_count.fetch_add(static_cast<std::uint64_t>(delta)) == 0) {
            my_parent->reserve();
        }
    }

    void release(std::uint32_t delta = 1) override {
        auto parent = my_parent;
        std::uint64_t ref = m_ref_count.fetch_sub(static_cast<std::uint64_t>(delta)) - static_cast<std::uint64_t>(delta);
        if (ref == 0) {
            parent->release();
        }
    }

    std::uint32_t get_num_child() {
        return static_cast<std::uint32_t>(m_ref_count.load(std::memory_order_acquire));
    }
private:
    wait_tree_vertex_interface* my_parent;
    std::atomic<std::uint64_t> m_ref_count;
};

struct execution_data {
    task_group_context* context{};
    slot_id original_slot{};
    slot_id affinity_slot{};
};

inline task_group_context* context(const execution_data& ed) {
    return ed.context;
}

inline slot_id original_slot(const execution_data& ed) {
    return ed.original_slot;
}

inline slot_id affinity_slot(const execution_data& ed) {
    return ed.affinity_slot;
}

inline slot_id execution_slot(const execution_data& ed) {
    return r1::execution_slot(&ed);
}

inline bool is_same_affinity(const execution_data& ed) {
    return affinity_slot(ed) == no_slot || affinity_slot(ed) == execution_slot(ed);
}

inline bool is_stolen(const execution_data& ed) {
    return original_slot(ed) != execution_slot(ed);
}

inline void spawn(task& t, task_group_context& ctx) {
    call_itt_task_notify(releasing, &t);
    r1::spawn(t, ctx);
}

inline void spawn(task& t, task_group_context& ctx, slot_id id) {
    call_itt_task_notify(releasing, &t);
    r1::spawn(t, ctx, id);
}

inline void execute_and_wait(task& t, task_group_context& t_ctx, wait_context& wait_ctx, task_group_context& w_ctx) {
    r1::execute_and_wait(t, t_ctx, wait_ctx, w_ctx);
    call_itt_task_notify(acquired, &wait_ctx);
    call_itt_task_notify(destroy, &wait_ctx);
}

inline void wait(wait_context& wait_ctx, task_group_context& ctx) {
    r1::wait(wait_ctx, ctx);
    call_itt_task_notify(acquired, &wait_ctx);
    call_itt_task_notify(destroy, &wait_ctx);
}

using r1::current_context;

class task_traits {
    std::uint64_t m_version_and_traits{};
    friend struct r1::task_accessor;
};

//! Alignment for a task object
static constexpr std::size_t task_alignment = 64;

//! Base class for user-defined tasks.
/** @ingroup task_scheduling */
class alignas(task_alignment) task : public task_traits {
protected:
    virtual ~task() = default;

public:
    virtual task* execute(execution_data&) = 0;
    virtual task* cancel(execution_data&) = 0;

private:
    std::uint64_t m_reserved[6]{};
    friend struct r1::task_accessor;
};
static_assert(sizeof(task) == task_alignment, "task size is broken");

} // namespace d1
} // namespace detail
} // namespace tbb

#endif /* __TBB__task_H */
