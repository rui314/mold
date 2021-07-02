/*
    Copyright (c) 2020-2021 Intel Corporation

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

#ifndef __TBB_thread_data_H
#define __TBB_thread_data_H

#include "oneapi/tbb/detail/_task.h"
#include "oneapi/tbb/task.h"

#include "rml_base.h" // rml::job

#include "scheduler_common.h"
#include "arena.h"
#include "concurrent_monitor.h"
#include "mailbox.h"
#include "misc.h" // FastRandom
#include "small_object_pool_impl.h"

#include <atomic>

namespace tbb {
namespace detail {
namespace r1 {

class task;
class arena_slot;
class task_group_context;
class task_dispatcher;

//------------------------------------------------------------------------
// Thread Data
//------------------------------------------------------------------------
class thread_data : public ::rml::job
                  , public intrusive_list_node
                  , no_copy {
public:
    thread_data(unsigned short index, bool is_worker)
        : my_arena_index{ index }
        , my_is_worker{ is_worker }
        , my_task_dispatcher{ nullptr }
        , my_arena{}
        , my_arena_slot{}
        , my_inbox{}
        , my_random{ this }
        , my_last_observer{ nullptr }
        , my_small_object_pool{new (cache_aligned_allocate(sizeof(small_object_pool_impl))) small_object_pool_impl{}}
        , my_context_list_state{}
#if __TBB_RESUMABLE_TASKS
        , my_post_resume_action{ post_resume_action::none }
        , my_post_resume_arg{nullptr}
#endif /* __TBB_RESUMABLE_TASKS */
    {
        ITT_SYNC_CREATE(&my_context_list_state.mutex, SyncType_Scheduler, SyncObj_ContextsList);
        my_context_list_state.head.next.store(&my_context_list_state.head, std::memory_order_relaxed);
        my_context_list_state.head.prev.store(&my_context_list_state.head, std::memory_order_relaxed);
    }

    ~thread_data() {
        context_list_cleanup();
        my_small_object_pool->destroy();
        poison_pointer(my_task_dispatcher);
        poison_pointer(my_arena);
        poison_pointer(my_arena_slot);
        poison_pointer(my_last_observer);
        poison_pointer(my_small_object_pool);
#if __TBB_RESUMABLE_TASKS
        poison_pointer(my_post_resume_arg);
#endif /* __TBB_RESUMABLE_TASKS */
        poison_value(my_context_list_state.epoch);
        poison_value(my_context_list_state.local_update);
        poison_value(my_context_list_state.nonlocal_update);
    }

    void attach_arena(arena& a, std::size_t index);
    bool is_attached_to(arena*);
    void attach_task_dispatcher(task_dispatcher&);
    void detach_task_dispatcher();
    void context_list_cleanup();
    template <typename T>
    void propagate_task_group_state(std::atomic<T> d1::task_group_context::* mptr_state, d1::task_group_context& src, T new_state);

    //! Index of the arena slot the scheduler occupies now, or occupied last time
    unsigned short my_arena_index;

    //! Indicates if the thread is created by RML
    const bool my_is_worker;

    //! The current task dipsatcher
    task_dispatcher* my_task_dispatcher;

    //! The arena that I own (if external thread) or am servicing at the moment (if worker)
    arena* my_arena;

    //! Pointer to the slot in the arena we own at the moment
    arena_slot* my_arena_slot;

    //! The mailbox (affinity mechanism) the current thread attached to
    mail_inbox my_inbox;

    //! The random generator
    FastRandom my_random;

    //! Last observer in the observers list processed on this slot
    observer_proxy* my_last_observer;

    //! Pool of small object for fast task allocation
    small_object_pool_impl* my_small_object_pool;

    struct context_list_state {
        //! Head of the thread specific list of task group contexts.
        d1::context_list_node head{};

        //! Mutex protecting access to the list of task group contexts.
        // TODO: check whether it can be deadly preempted and replace by spinning/sleeping mutex
        spin_mutex mutex{};

        //! Last state propagation epoch known to this thread
        /** Together with the_context_state_propagation_epoch constitute synchronization protocol
        that keeps hot path of task group context construction destruction mostly
        lock-free.
        When local epoch equals the global one, the state of task group contexts
        registered with this thread is consistent with that of the task group trees
        they belong to. **/
        std::atomic<std::uintptr_t> epoch{};

        //! Flag indicating that a context is being destructed by its owner thread
        /** Together with my_nonlocal_ctx_list_update constitute synchronization protocol
        that keeps hot path of context destruction (by the owner thread) mostly
        lock-free. **/
        std::atomic<std::uintptr_t> local_update{};

        //! Flag indicating that a context is being destructed by non-owner thread.
        /** See also my_local_update. **/
        std::atomic<std::uintptr_t> nonlocal_update{};
    } my_context_list_state;

#if __TBB_RESUMABLE_TASKS
    //! The list of possible post resume actions.
    enum class post_resume_action {
        invalid,
        register_waiter,
        resume,
        callback,
        cleanup,
        notify,
        none
    };

    //! The callback to call the user callback passed to tbb::suspend.
    struct suspend_callback_wrapper {
        suspend_callback_type suspend_callback;
        void* user_callback;
        suspend_point_type* tag;

        void operator()() {
            __TBB_ASSERT(suspend_callback && user_callback && tag, nullptr);
            suspend_callback(user_callback, tag);
        }
    };

    //! Suspends the current coroutine (task_dispatcher).
    void suspend(void* suspend_callback, void* user_callback);

    //! Resumes the target task_dispatcher.
    void resume(task_dispatcher& target);

    //! Set post resume action to perform after resume.
    void set_post_resume_action(post_resume_action pra, void* arg) {
        __TBB_ASSERT(my_post_resume_action == post_resume_action::none, "The Post resume action must not be set");
        __TBB_ASSERT(!my_post_resume_arg, "The post resume action must not have an argument");
        my_post_resume_action = pra;
        my_post_resume_arg = arg;
    }

    void clear_post_resume_action() {
        my_post_resume_action = thread_data::post_resume_action::none;
        my_post_resume_arg = nullptr;
    }

    //! Performs post resume action.
    void do_post_resume_action();

    //! The post resume action requested after the swap contexts.
    post_resume_action my_post_resume_action;

    //! The post resume action argument.
    void* my_post_resume_arg;
#endif /* __TBB_RESUMABLE_TASKS */

    //! The default context
    // TODO: consider using common default context because it is used only to simplify
    // cancellation check.
    d1::task_group_context my_default_context;
};

inline void thread_data::attach_arena(arena& a, std::size_t index) {
    my_arena = &a;
    my_arena_index = static_cast<unsigned short>(index);
    my_arena_slot = a.my_slots + index;
    // Read the current slot mail_outbox and attach it to the mail_inbox (remove inbox later maybe)
    my_inbox.attach(my_arena->mailbox(index));
}

inline bool thread_data::is_attached_to(arena* a) { return my_arena == a; }

inline void thread_data::context_list_cleanup() {
    // Detach contexts remaining in the local list.
    {
        spin_mutex::scoped_lock lock(my_context_list_state.mutex);
        d1::context_list_node* node = my_context_list_state.head.next.load(std::memory_order_relaxed);
        while (node != &my_context_list_state.head) {
            using state_t = d1::task_group_context::lifetime_state;

            d1::task_group_context& ctx = __TBB_get_object_ref(d1::task_group_context, my_node, node);
            std::atomic<state_t>& state = ctx.my_lifetime_state;

            node = node->next.load(std::memory_order_relaxed);

            __TBB_ASSERT(ctx.my_owner == this, "The context should belong to the current thread.");
            state_t expected = state_t::bound;
            if (
#if defined(__INTEL_COMPILER) && __INTEL_COMPILER <= 1910
                !((std::atomic<typename std::underlying_type<state_t>::type>&)state).compare_exchange_strong(
                    (typename std::underlying_type<state_t>::type&)expected,
                    (typename std::underlying_type<state_t>::type)state_t::detached)
#else
                !state.compare_exchange_strong(expected, state_t::detached)
#endif
            ) {
                __TBB_ASSERT(expected == state_t::locked || expected == state_t::dying, nullptr);
                spin_wait_until_eq(state, state_t::dying);
            } else {
                __TBB_ASSERT(expected == state_t::bound, nullptr);
                ctx.my_owner.store(nullptr, std::memory_order_release);
            }
        }
    }
    spin_wait_until_eq(my_context_list_state.nonlocal_update, 0u);
}

inline void thread_data::attach_task_dispatcher(task_dispatcher& task_disp) {
    __TBB_ASSERT(my_task_dispatcher == nullptr, nullptr);
    __TBB_ASSERT(task_disp.m_thread_data == nullptr, nullptr);
    task_disp.m_thread_data = this;
    my_task_dispatcher = &task_disp;
}

inline void thread_data::detach_task_dispatcher() {
    __TBB_ASSERT(my_task_dispatcher != nullptr, nullptr);
    __TBB_ASSERT(my_task_dispatcher->m_thread_data == this, nullptr);
    my_task_dispatcher->m_thread_data = nullptr;
    my_task_dispatcher = nullptr;
}

} // namespace r1
} // namespace detail
} // namespace tbb

#endif // __TBB_thread_data_H

