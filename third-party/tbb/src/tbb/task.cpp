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
    __TBB_ASSERT(task_disp.m_thread_data == nullptr, nullptr);

    // TODO: remove this work-around
    // Prolong the arena's lifetime while all coroutines are alive
    // (otherwise the arena can be destroyed while some tasks are suspended).
    arena& a = *sp->m_arena;
    a.my_references += arena::ref_external;

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
    a.on_thread_leaving<arena::ref_external>();
}

suspend_point_type* current_suspend_point() {
    thread_data& td = *governor::get_thread_data();
    return td.my_task_dispatcher->get_suspend_point();
}

static task_dispatcher& create_coroutine(thread_data& td) {
    // We may have some task dispatchers cached
    task_dispatcher* task_disp = td.my_arena->my_co_cache.pop();
    if (!task_disp) {
        void* ptr = cache_aligned_allocate(sizeof(task_dispatcher));
        task_disp = new(ptr) task_dispatcher(td.my_arena);
        task_disp->init_suspend_point(td.my_arena, td.my_arena->my_market->worker_stack_size());
    }
    // Prolong the arena's lifetime until all coroutines is alive
    // (otherwise the arena can be destroyed while some tasks are suspended).
    // TODO: consider behavior if there are more than 4K external references.
    td.my_arena->my_references += arena::ref_external;
    return *task_disp;
}

void task_dispatcher::suspend(suspend_callback_type suspend_callback, void* user_callback) {
    __TBB_ASSERT(suspend_callback != nullptr, nullptr);
    __TBB_ASSERT(user_callback != nullptr, nullptr);
    __TBB_ASSERT(m_thread_data != nullptr, nullptr);

    arena_slot* slot = m_thread_data->my_arena_slot;
    __TBB_ASSERT(slot != nullptr, nullptr);

    task_dispatcher& default_task_disp = slot->default_task_dispatcher();
    // TODO: simplify the next line, e.g. is_task_dispatcher_recalled( task_dispatcher& )
    bool is_recalled = default_task_disp.get_suspend_point()->m_is_owner_recalled.load(std::memory_order_acquire);
    task_dispatcher& target = is_recalled ? default_task_disp : create_coroutine(*m_thread_data);

    thread_data::suspend_callback_wrapper callback = { suspend_callback, user_callback, get_suspend_point() };
    m_thread_data->set_post_resume_action(thread_data::post_resume_action::callback, &callback);
    resume(target);

    if (m_properties.outermost) {
        recall_point();
    }
}

bool task_dispatcher::resume(task_dispatcher& target) {
    // Do not create non-trivial objects on the stack of this function. They might never be destroyed
    {
        thread_data* td = m_thread_data;
        __TBB_ASSERT(&target != this, "We cannot resume to ourself");
        __TBB_ASSERT(td != nullptr, "This task dispatcher must be attach to a thread data");
        __TBB_ASSERT(td->my_task_dispatcher == this, "Thread data must be attached to this task dispatcher");
        __TBB_ASSERT(td->my_post_resume_action != thread_data::post_resume_action::none, "The post resume action must be set");
        __TBB_ASSERT(td->my_post_resume_arg, "The post resume action must have an argument");

        // Change the task dispatcher
        td->detach_task_dispatcher();
        td->attach_task_dispatcher(target);
    }
    __TBB_ASSERT(m_suspend_point != nullptr, "Suspend point must be created");
    __TBB_ASSERT(target.m_suspend_point != nullptr, "Suspend point must be created");
    // Swap to the target coroutine.
    m_suspend_point->m_co_context.resume(target.m_suspend_point->m_co_context);
    // Pay attention that m_thread_data can be changed after resume
    if (m_thread_data) {
        thread_data* td = m_thread_data;
        __TBB_ASSERT(td != nullptr, "This task dispatcher must be attach to a thread data");
        __TBB_ASSERT(td->my_task_dispatcher == this, "Thread data must be attached to this task dispatcher");
        td->do_post_resume_action();

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

void thread_data::do_post_resume_action() {
    __TBB_ASSERT(my_post_resume_action != thread_data::post_resume_action::none, "The post resume action must be set");
    __TBB_ASSERT(my_post_resume_arg, "The post resume action must have an argument");

    switch (my_post_resume_action) {
    case post_resume_action::register_waiter:
    {
        static_cast<market_concurrent_monitor::resume_context*>(my_post_resume_arg)->notify();
        break;
    }
    case post_resume_action::resume:
    {
        r1::resume(static_cast<suspend_point_type*>(my_post_resume_arg));
        break;
    }
    case post_resume_action::callback:
    {
        suspend_callback_wrapper callback = *static_cast<suspend_callback_wrapper*>(my_post_resume_arg);
        callback();
        break;
    }
    case post_resume_action::cleanup:
    {
        task_dispatcher* to_cleanup = static_cast<task_dispatcher*>(my_post_resume_arg);
        // Release coroutine's reference to my_arena
        my_arena->on_thread_leaving<arena::ref_external>();
        // Cache the coroutine for possible later re-usage
        my_arena->my_co_cache.push(to_cleanup);
        break;
    }
    case post_resume_action::notify:
    {
        suspend_point_type* sp = static_cast<suspend_point_type*>(my_post_resume_arg);
        sp->m_is_owner_recalled.store(true, std::memory_order_release);
        // Do not access sp because it can be destroyed after the store

        auto is_our_suspend_point = [sp](market_context ctx) {
            return  std::uintptr_t(sp) == ctx.my_uniq_addr;
        };
        my_arena->my_market->get_wait_list().notify(is_our_suspend_point);
        break;
    }
    default:
        __TBB_ASSERT(false, "Unknown post resume action");
    }

    my_post_resume_action = post_resume_action::none;
    my_post_resume_arg = nullptr;
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

    r1::governor::get_thread_data()->my_arena->my_market->get_wait_list().notify(is_related_wait_ctx);
}

} // namespace r1
} // namespace detail
} // namespace tbb

