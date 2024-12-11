/*
    Copyright (c) 2005-2024 Intel Corporation

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

// Do not include task.h directly. Use scheduler_common.h instead
#include "scheduler_common.h"
#include "governor.h"
#include "arena.h"
#include "thread_data.h"
#include "task_dispatcher.h"
#include "waiters.h"
#include "itt_notify.h"

#include "oneapi/tbb/detail/_task.h"
#include "oneapi/tbb/partitioner.h"
#include "oneapi/tbb/task.h"

#include <cstring>

namespace tbb {
namespace detail {
namespace r1 {

//------------------------------------------------------------------------
// resumable tasks
//------------------------------------------------------------------------
#if __TBB_RESUMABLE_TASKS

void suspend(suspend_callback_type suspend_callback, void* user_callback) {
    thread_data& td = *governor::get_thread_data();
    td.my_task_dispatcher->suspend(suspend_callback, user_callback);
    // Do not access td after suspend.
}

void resume(suspend_point_type* sp) {
    assert_pointers_valid(sp, sp->m_arena);
    task_dispatcher& task_disp = sp->m_resume_task.m_target;

    if (sp->try_notify_resume()) {
        // TODO: remove this work-around
        // Prolong the arena's lifetime while all coroutines are alive
        // (otherwise the arena can be destroyed while some tasks are suspended).
        arena& a = *sp->m_arena;
        a.my_references += arena::ref_worker;

        if (task_disp.m_properties.critical_task_allowed) {
            // The target is not in the process of executing critical task, so the resume task is not critical.
            a.my_resume_task_stream.push(&sp->m_resume_task, random_lane_selector(sp->m_random));
        } else {
    #if __TBB_PREVIEW_CRITICAL_TASKS
            // The target is in the process of executing critical task, so the resume task is critical.
            a.my_critical_task_stream.push(&sp->m_resume_task, random_lane_selector(sp->m_random));
    #endif
        }
        // Do not access target after that point.
        a.advertise_new_work<arena::wakeup>();
        // Release our reference to my_arena.
        a.on_thread_leaving(arena::ref_worker);
    }

}

suspend_point_type* current_suspend_point() {
    thread_data& td = *governor::get_thread_data();
    return td.my_task_dispatcher->get_suspend_point();
}

task_dispatcher& create_coroutine(thread_data& td) {
    // We may have some task dispatchers cached
    task_dispatcher* task_disp = td.my_arena->my_co_cache.pop();
    if (!task_disp) {
        void* ptr = cache_aligned_allocate(sizeof(task_dispatcher));
        task_disp = new(ptr) task_dispatcher(td.my_arena);
        task_disp->init_suspend_point(td.my_arena, td.my_arena->my_threading_control->worker_stack_size());
    }
    // Prolong the arena's lifetime until all coroutines is alive
    // (otherwise the arena can be destroyed while some tasks are suspended).
    // TODO: consider behavior if there are more than 4K external references.
    td.my_arena->my_references += arena::ref_external;
    return *task_disp;
}

void task_dispatcher::internal_suspend() {
    __TBB_ASSERT(m_thread_data != nullptr, nullptr);

    arena_slot* slot = m_thread_data->my_arena_slot;
    __TBB_ASSERT(slot != nullptr, nullptr);

    task_dispatcher& default_task_disp = slot->default_task_dispatcher();
    // TODO: simplify the next line, e.g. is_task_dispatcher_recalled( task_dispatcher& )
    bool is_recalled = default_task_disp.get_suspend_point()->m_is_owner_recalled.load(std::memory_order_acquire);
    task_dispatcher& target = is_recalled ? default_task_disp : create_coroutine(*m_thread_data);

    resume(target);

    if (m_properties.outermost) {
        recall_point();
    }
}

void task_dispatcher::suspend(suspend_callback_type suspend_callback, void* user_callback) {
    __TBB_ASSERT(suspend_callback != nullptr, nullptr);
    __TBB_ASSERT(user_callback != nullptr, nullptr);
    suspend_callback(user_callback, get_suspend_point());

    __TBB_ASSERT(m_thread_data != nullptr, nullptr);
    __TBB_ASSERT(m_thread_data->my_post_resume_action == post_resume_action::none, nullptr);
    __TBB_ASSERT(m_thread_data->my_post_resume_arg == nullptr, nullptr);
    internal_suspend();
}

bool task_dispatcher::resume(task_dispatcher& target) {
    // Do not create non-trivial objects on the stack of this function. They might never be destroyed
    {
        thread_data* td = m_thread_data;
        __TBB_ASSERT(&target != this, "We cannot resume to ourself");
        __TBB_ASSERT(td != nullptr, "This task dispatcher must be attach to a thread data");
        __TBB_ASSERT(td->my_task_dispatcher == this, "Thread data must be attached to this task dispatcher");

        // Change the task dispatcher
        td->detach_task_dispatcher();
        td->attach_task_dispatcher(target);
    }
    __TBB_ASSERT(m_suspend_point != nullptr, "Suspend point must be created");
    __TBB_ASSERT(target.m_suspend_point != nullptr, "Suspend point must be created");
    // Swap to the target coroutine.

    m_suspend_point->resume(target.m_suspend_point);
    // Pay attention that m_thread_data can be changed after resume
    if (m_thread_data) {
        thread_data* td = m_thread_data;
        __TBB_ASSERT(td != nullptr, "This task dispatcher must be attach to a thread data");
        __TBB_ASSERT(td->my_task_dispatcher == this, "Thread data must be attached to this task dispatcher");
        do_post_resume_action();

        // Remove the recall flag if the thread in its original task dispatcher
        arena_slot* slot = td->my_arena_slot;
        __TBB_ASSERT(slot != nullptr, nullptr);
        if (this == slot->my_default_task_dispatcher) {
            __TBB_ASSERT(m_suspend_point != nullptr, nullptr);
            m_suspend_point->m_is_owner_recalled.store(false, std::memory_order_relaxed);
        }
        return true;
    }
    return false;
}

void task_dispatcher::do_post_resume_action() {
    thread_data* td = m_thread_data;
    switch (td->my_post_resume_action) {
    case post_resume_action::register_waiter:
    {
        __TBB_ASSERT(td->my_post_resume_arg, "The post resume action must have an argument");
        static_cast<thread_control_monitor::resume_context*>(td->my_post_resume_arg)->notify();
        break;
    }
    case post_resume_action::cleanup:
    {
        __TBB_ASSERT(td->my_post_resume_arg, "The post resume action must have an argument");
        task_dispatcher* to_cleanup = static_cast<task_dispatcher*>(td->my_post_resume_arg);
        // Release coroutine's reference to my_arena
        td->my_arena->on_thread_leaving(arena::ref_external);
        // Cache the coroutine for possible later re-usage
        td->my_arena->my_co_cache.push(to_cleanup);
        break;
    }
    case post_resume_action::notify:
    {
        __TBB_ASSERT(td->my_post_resume_arg, "The post resume action must have an argument");
        suspend_point_type* sp = static_cast<suspend_point_type*>(td->my_post_resume_arg);
        sp->recall_owner();
        // Do not access sp because it can be destroyed after recall

        auto is_our_suspend_point = [sp] (market_context ctx) {
            return std::uintptr_t(sp) == ctx.my_uniq_addr;
        };
        td->my_arena->get_waiting_threads_monitor().notify(is_our_suspend_point);
        break;
    }
    default:
        __TBB_ASSERT(td->my_post_resume_action == post_resume_action::none, "Unknown post resume action");
        __TBB_ASSERT(td->my_post_resume_arg == nullptr, "The post resume argument should not be set");
    }
    td->clear_post_resume_action();
}

#else

void suspend(suspend_callback_type, void*) {
    __TBB_ASSERT_RELEASE(false, "Resumable tasks are unsupported on this platform");
}

void resume(suspend_point_type*) {
    __TBB_ASSERT_RELEASE(false, "Resumable tasks are unsupported on this platform");
}

suspend_point_type* current_suspend_point() {
    __TBB_ASSERT_RELEASE(false, "Resumable tasks are unsupported on this platform");
    return nullptr;
}

#endif /* __TBB_RESUMABLE_TASKS */

void notify_waiters(std::uintptr_t wait_ctx_addr) {
    auto is_related_wait_ctx = [&] (market_context context) {
        return wait_ctx_addr == context.my_uniq_addr;
    };

    governor::get_thread_data()->my_arena->get_waiting_threads_monitor().notify(is_related_wait_ctx);
}

d1::wait_tree_vertex_interface* get_thread_reference_vertex(d1::wait_tree_vertex_interface* top_wait_context) {
    __TBB_ASSERT(top_wait_context, nullptr);
    auto& dispatcher = *governor::get_thread_data()->my_task_dispatcher;

    d1::reference_vertex* ref_counter{nullptr};
    auto& reference_map = dispatcher.m_reference_vertex_map;
    auto pos = reference_map.find(top_wait_context);
    if (pos != reference_map.end()) {
        ref_counter = pos->second;
    } else {
        constexpr std::size_t max_reference_vertex_map_size = 1000;
        if (reference_map.size() > max_reference_vertex_map_size) {
            // TODO: Research the possibility of using better approach for a clean-up
            for (auto it = reference_map.begin(); it != reference_map.end();) {
                if (it->second->get_num_child() == 0) {
                    it->second->~reference_vertex();
                    cache_aligned_deallocate(it->second);
                    it = reference_map.erase(it);
                } else {
                    ++it;
                }
            }
        }

        reference_map[top_wait_context] = ref_counter =
            new (cache_aligned_allocate(sizeof(d1::reference_vertex))) d1::reference_vertex(top_wait_context, 0);
    }

    return ref_counter;
}

} // namespace r1
} // namespace detail
} // namespace tbb
