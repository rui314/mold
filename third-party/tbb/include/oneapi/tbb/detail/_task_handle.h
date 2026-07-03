/*
    Copyright (c) 2020-2025 Intel Corporation
    Copyright (c) 2025 UXL Foundation Contributors

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


#ifndef __TBB_task_handle_H
#define __TBB_task_handle_H

#include "_config.h"
#include "_task.h"
#include "_small_object_pool.h"
#include "_utils.h"
#include <memory>

namespace tbb {
namespace detail {

namespace d1 { class task_group_context; class wait_context; struct execution_data; }
namespace d2 {

class task_handle;

enum task_group_status {
    not_complete,
    complete,
    canceled
#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS
    , task_complete
#endif
};

#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS

class task_handle_task;
class task_dynamic_state;

struct notify_list_node {
    // First element is a pointer to a bypassed task
    // Second element is true if bypass is allowed
    using notify_result_type = std::pair<task_handle_task*, bool>;

    notify_list_node* next_node = nullptr;

    virtual notify_result_type notify_on_completion() = 0;
    virtual notify_result_type notify_on_cancellation() = 0;
    virtual ~notify_list_node() = default;
};

struct notify_successor_node : notify_list_node {
    task_dynamic_state* successor_state = nullptr;
    d1::small_object_allocator allocator;

    notify_successor_node(task_dynamic_state* state, d1::small_object_allocator& alloc)
        : successor_state(state), allocator(alloc) {}

    notify_result_type notify_common();

    notify_result_type notify_on_completion() override {
        return notify_common();
    }

    notify_result_type notify_on_cancellation() override {
        return notify_common();
    }
};

struct notify_waiter_node : notify_list_node {
    d1::wait_context task_wait_context;
    bool was_canceled;

    notify_waiter_node() : task_wait_context(1), was_canceled(false) {}

    notify_result_type notify_common() {
        task_wait_context.release();

        // Bypassing from list notification is not allowed if there are waiters in the list
        return {nullptr, false};
    }

    virtual notify_result_type notify_on_completion() override {
        return notify_common();
    }

    virtual notify_result_type notify_on_cancellation() override {
        was_canceled = true;
        return notify_common();
    }
};

class task_dynamic_state {
public:
    task_dynamic_state(task_handle_task* task, d1::small_object_allocator& alloc)
        : m_task(task)
        , m_notify_list_head(nullptr)
        , m_new_completion_point(nullptr)
        , m_num_dependencies(0)
        , m_num_references(1) // reserves a task co-ownership for dynamic state
        , m_allocator(alloc)
    {}

    void reserve() { ++m_num_references; }

    void release() {
        if (--m_num_references == 0) {
            task_dynamic_state* new_completion_point = m_new_completion_point.load(std::memory_order_relaxed);
            // There was a new completion point assigned to the current one by transferring the completion
            // Need to unregister the current dynamic state as a co-owner
            if (new_completion_point) new_completion_point->release();
            m_allocator.delete_object(this);
        }
    }

    void register_dependency() {
        if (m_num_dependencies++ == 0) {
            // Register an additional dependency for a task_handle owning the current task
            ++m_num_dependencies;
        }
    }

    // Returns true if the released dependency was the last remaining one; false otherwise
    bool release_dependency() {
        auto updated_dependency_counter = --m_num_dependencies;
        return updated_dependency_counter == 0;
    }

    bool has_dependencies() const {
        return m_num_dependencies.load(std::memory_order_acquire) != 0;
    }

    task_handle_task* complete_and_try_get_successor();
    task_handle_task* cancel_and_try_get_successor();

    void add_successor(task_handle&  successor);
    task_group_status wait_for_completion(d1::task_group_context&);
    task_group_status run_self_and_wait_for_completion(d1::task_group_context&);
    void add_notify_node(notify_list_node* new_notify_node, notify_list_node* current_notify_list_head);
    void add_notify_list(notify_list_node* notify_list);

    using notify_list_state_flag = std::uintptr_t;
    static constexpr notify_list_state_flag COMPLETED_FLAG = ~std::uintptr_t(0);
    static constexpr notify_list_state_flag TRANSFERRED_FLAG = ~std::uintptr_t(1);
    static constexpr notify_list_state_flag CANCELED_FLAG = ~std::uintptr_t(2);

    static bool represents_completed_task(notify_list_node* list_head) {
        return list_head == reinterpret_cast<notify_list_node*>(COMPLETED_FLAG);
    }

    static bool represents_canceled_task(notify_list_node* list_head) {
        return list_head == reinterpret_cast<notify_list_node*>(CANCELED_FLAG);
    }

    static bool represents_transferred_completion(notify_list_node* list_head) {
        return list_head == reinterpret_cast<notify_list_node*>(TRANSFERRED_FLAG);
    }

    task_handle_task* fetch_list_and_notify_all(notify_list_state_flag);

    notify_list_node* fetch_notify_list(notify_list_state_flag new_list_state_flag) {
        return m_notify_list_head.exchange(reinterpret_cast<notify_list_node*>(new_list_state_flag));
    }

    void transfer_completion_to(task_dynamic_state* new_completion_point) {
        __TBB_ASSERT(new_completion_point != nullptr, nullptr);
        // Register current dynamic state as a co-owner of the new_completion_point
        // to prevent it's early destruction
        new_completion_point->reserve();
        m_new_completion_point.store(new_completion_point, std::memory_order_relaxed);
        notify_list_node* notify_list = fetch_notify_list(TRANSFERRED_FLAG);
        new_completion_point->add_notify_list(notify_list);
    }

    task_group_status get_task_status() {
        notify_list_node* current_list_head = m_notify_list_head.load(std::memory_order_acquire);
        task_group_status status = task_group_status::not_complete;

        if (represents_transferred_completion(current_list_head)) {
            task_dynamic_state* new_completion_point = m_new_completion_point.load(std::memory_order_relaxed);
            __TBB_ASSERT(new_completion_point != nullptr, nullptr);
            status = new_completion_point->get_task_status();
        } else if (represents_completed_task(current_list_head)) {
            status = task_group_status::task_complete;
        } else if (represents_canceled_task(current_list_head)) {
            status = task_group_status::canceled;
        }

        return status;
    }

    task_handle_task* get_task() { return m_task; }

private:
    task_handle_task* m_task;
    std::atomic<notify_list_node*> m_notify_list_head;
    std::atomic<task_dynamic_state*> m_new_completion_point;
    std::atomic<std::size_t> m_num_dependencies;
    std::atomic<std::size_t> m_num_references;
    d1::small_object_allocator m_allocator;
};

inline std::pair<task_handle_task*, bool> notify_successor_node::notify_common() {
    task_handle_task* successor_task = nullptr;
    if (successor_state->release_dependency()) {
        successor_task = successor_state->get_task();
        allocator.delete_object(this);
    }

    return {successor_task, true};
}

class dynamic_state_task : public d1::task {
    std::atomic<task_dynamic_state*> m_dynamic_state;
public:
    dynamic_state_task() : m_dynamic_state(nullptr) {}

    ~dynamic_state_task() {
        task_dynamic_state* current_state = m_dynamic_state.load(std::memory_order_relaxed);
        if (current_state != nullptr) {
            current_state->release();
        }
    }

    // Returns the dynamic state associated with the task. If the state has not been initialized, initializes it.
    task_dynamic_state* get_dynamic_state();

    task_handle_task* complete_and_try_get_successor() {
        task_handle_task* next_task = nullptr;

        task_dynamic_state* current_state = m_dynamic_state.load(std::memory_order_relaxed);
        if (current_state != nullptr) {
            next_task = current_state->complete_and_try_get_successor();
        }
        return next_task;
    }

    task_handle_task* cancel_and_try_get_successor() {
        task_handle_task* next_task = nullptr;

        task_dynamic_state* current_state = m_dynamic_state.load(std::memory_order_relaxed);
        if (current_state != nullptr) {
            next_task = current_state->cancel_and_try_get_successor();
        }
        return next_task;
    }

    // Returns true if the released dependency was the last remaining one; false otherwise
    bool release_dependency() {
        task_dynamic_state* current_state = m_dynamic_state.load(std::memory_order_relaxed);
        __TBB_ASSERT(current_state != nullptr && current_state->has_dependencies(),
                     "release_dependency was called for task without dependencies");
        return current_state->release_dependency();
    }

    bool has_dependencies() const {
        task_dynamic_state* current_state = m_dynamic_state.load(std::memory_order_relaxed);
        return current_state ? current_state->has_dependencies() : false;
    }

    void transfer_completion_to(task_handle& receiving_task);
};
#endif // __TBB_PREVIEW_TASK_GROUP_EXTENSIONS

class task_handle_task
#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS
    : public dynamic_state_task
#else
    : public d1::task
#endif
{
    // Pointer to the instantiation of destroy_function_task with the concrete derived type,
    // used for correct destruction and deallocation of the task
    using destroy_func_type = void (*)(task_handle_task*, d1::small_object_allocator&, const d1::execution_data*);

    // Reuses the first std::uint64_t field (previously m_version_and_traits) to maintain backward compatibility
    // The type of the first field remains std::uint64_t to preserve alignment and offset of subsequent member variables.
    static_assert(sizeof(destroy_func_type) <= sizeof(std::uint64_t), "Cannot fit destroy pointer into std::uint64_t");
    std::uint64_t m_destroy_func;

    d1::wait_tree_vertex_interface* m_wait_tree_vertex;
    d1::task_group_context& m_ctx;
    d1::small_object_allocator m_allocator;
public:
    void destroy(const d1::execution_data* ed = nullptr) {
        destroy_func_type destroy_func = reinterpret_cast<destroy_func_type>(m_destroy_func);
        if (destroy_func != nullptr) {
            // If the destroy function is set for the current instantiation - use it
            (*destroy_func)(this, m_allocator, ed);
        } else {
            // Otherwise, the object was compiled with the old version of the library
            // Destroy the object and let the memory leak since the derived type is unknown
            // and the object cannot be deallocated properly
            this->~task_handle_task();
        }
    }

    task_handle_task(d1::wait_tree_vertex_interface* vertex, d1::task_group_context& ctx,
                     d1::small_object_allocator& alloc, destroy_func_type destroy_func)
        : m_destroy_func(reinterpret_cast<std::uint64_t>(destroy_func))
        , m_wait_tree_vertex(vertex)
        , m_ctx(ctx)
        , m_allocator(alloc)
    {
        m_wait_tree_vertex->reserve();
    }

    ~task_handle_task() override {
        m_wait_tree_vertex->release();
    }

    d1::task_group_context& ctx() const { return m_ctx; }
};

#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS
inline task_dynamic_state* dynamic_state_task::get_dynamic_state() {
#if __TBB_USE_OPTIONAL_RTTI
    __TBB_ASSERT(dynamic_cast<task_handle_task*>(this) != nullptr, "get_dynamic_state was called for a stack task");
#endif
    task_dynamic_state* current_state = m_dynamic_state.load(std::memory_order_acquire);

    if (current_state == nullptr) {
        d1::small_object_allocator alloc;

        task_dynamic_state* new_state = alloc.new_object<task_dynamic_state>(static_cast<task_handle_task*>(this), alloc);

        if (m_dynamic_state.compare_exchange_strong(current_state, new_state)) {
            current_state = new_state;
        } else {
            // CAS failed, current_state points to the dynamic state created by another thread
            alloc.delete_object(new_state);
        }
    }

    __TBB_ASSERT(current_state != nullptr, "Failed to create dynamic state");
    return current_state;
}
#endif

class task_handle {
    struct task_handle_task_deleter {
        void operator()(task_handle_task* p){ p->destroy(); }
    };
    using handle_impl_t = std::unique_ptr<task_handle_task, task_handle_task_deleter>;

    handle_impl_t m_handle = {nullptr};
public:
    task_handle() = default;
    task_handle(task_handle&&) = default;
    task_handle& operator=(task_handle&&) = default;

    explicit operator bool() const noexcept { return static_cast<bool>(m_handle); }

    friend bool operator==(task_handle const& th, std::nullptr_t) noexcept;
    friend bool operator==(std::nullptr_t, task_handle const& th) noexcept;

    friend bool operator!=(task_handle const& th, std::nullptr_t) noexcept;
    friend bool operator!=(std::nullptr_t, task_handle const& th) noexcept;

private:
    friend struct task_handle_accessor;
#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS
    friend class task_completion_handle;
#endif

    task_handle(task_handle_task* t) : m_handle {t}{}
};

struct task_handle_accessor {
    static task_handle construct(task_handle_task* t) { return {t}; }

    static task_handle_task* release(task_handle& th) {
        return th.m_handle.release();
    }

    static d1::task_group_context& ctx_of(task_handle& th) {
        __TBB_ASSERT(th.m_handle, "ctx_of does not expect empty task_handle.");
        return th.m_handle->ctx();
    }

#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS
    static task_dynamic_state* get_task_dynamic_state(task_handle& th) {
        return th.m_handle->get_dynamic_state();
    }
#endif
};

inline bool operator==(task_handle const& th, std::nullptr_t) noexcept {
    return th.m_handle == nullptr;
}
inline bool operator==(std::nullptr_t, task_handle const& th) noexcept {
    return th.m_handle == nullptr;
}

inline bool operator!=(task_handle const& th, std::nullptr_t) noexcept {
    return th.m_handle != nullptr;
}

inline bool operator!=(std::nullptr_t, task_handle const& th) noexcept {
    return th.m_handle != nullptr;
}

#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS
inline void task_dynamic_state::add_notify_node(notify_list_node* new_notify_node,
                                                notify_list_node* current_notify_list_head)
{
    __TBB_ASSERT(new_notify_node != nullptr, nullptr);

    new_notify_node->next_node = current_notify_list_head;

    while (!m_notify_list_head.compare_exchange_strong(current_notify_list_head, new_notify_node)) {
        // Other thread updated the head of the list

        if (represents_completed_task(current_notify_list_head)) {
            // Current task has completed while we tried to insert the node to the list
            new_notify_node->notify_on_completion();
            break;
        } else if (represents_canceled_task(current_notify_list_head)) {
            // Current task has canceled while we tried to insert the node to the list
            new_notify_node->notify_on_cancellation();
            break;
        } else if (represents_transferred_completion(current_notify_list_head)) {
            // Redirect notify_node to the task received the completion
            task_dynamic_state* new_completion_point = m_new_completion_point.load(std::memory_order_relaxed);
            __TBB_ASSERT(new_completion_point, "notify list is marked as transferred, but new dynamic state is not set");
            new_completion_point->add_notify_node(new_notify_node, new_completion_point->m_notify_list_head.load(std::memory_order_acquire));
            break;
        }

        new_notify_node->next_node = current_notify_list_head;
    }
}


inline void task_dynamic_state::add_successor(task_handle& successor) {
    notify_list_node* current_notify_list_head = m_notify_list_head.load(std::memory_order_acquire);

    if (!represents_completed_task(current_notify_list_head) && !represents_canceled_task(current_notify_list_head)) {
        if (represents_transferred_completion(current_notify_list_head)) {
            // Redirect successor to the task received the completion
            task_dynamic_state* new_completion_point = m_new_completion_point.load(std::memory_order_relaxed);
            __TBB_ASSERT(new_completion_point, "notify list is marked as transferred, but new dynamic state is not set");
            new_completion_point->add_successor(successor);
        } else {
            task_dynamic_state* successor_state = task_handle_accessor::get_task_dynamic_state(successor);
            successor_state->register_dependency();
    
            d1::small_object_allocator alloc;
            notify_successor_node* new_successor_node = alloc.new_object<notify_successor_node>(successor_state, alloc);
            add_notify_node(new_successor_node, current_notify_list_head);
        }
    }
}

inline task_group_status task_dynamic_state::wait_for_completion(d1::task_group_context& ctx) {
    notify_list_node* current_notify_list_head = m_notify_list_head.load(std::memory_order_acquire);
    task_group_status status = task_group_status::not_complete;

    if (represents_completed_task(current_notify_list_head)) {
        status = task_group_status::task_complete;
    } else if (represents_canceled_task(current_notify_list_head)) {
        status = task_group_status::canceled;
    } else if (represents_transferred_completion(current_notify_list_head)) {
        // Redirect waiter to the task received the completion
        task_dynamic_state* new_completion_point = m_new_completion_point.load(std::memory_order_relaxed);
        __TBB_ASSERT(new_completion_point, "notify list is marked as transferred, but new dynamic state is not set");
        status = new_completion_point->wait_for_completion(ctx);
    } else {
        notify_waiter_node waiter_node;
        add_notify_node(&waiter_node, current_notify_list_head);
        d1::wait(waiter_node.task_wait_context, ctx);
        status = waiter_node.was_canceled ? task_group_status::canceled : task_group_status::task_complete;
    }
    
    return status;
}

inline task_group_status task_dynamic_state::run_self_and_wait_for_completion(d1::task_group_context& ctx) {
    __TBB_ASSERT(!has_dependencies(), nullptr);
    notify_list_node* current_notify_list_head = m_notify_list_head.load(std::memory_order_acquire);

    __TBB_ASSERT(!represents_completed_task(current_notify_list_head), "non-submitted task cannot be completed");
    __TBB_ASSERT(!represents_canceled_task(current_notify_list_head), "non-submitted task cannot be canceled");
    __TBB_ASSERT(!represents_transferred_completion(current_notify_list_head), "non-submitted task completion cannot be transferred");

    notify_waiter_node waiter_node;
    add_notify_node(&waiter_node, current_notify_list_head);
    d1::execute_and_wait(*get_task(), ctx, waiter_node.task_wait_context, ctx);
    return waiter_node.was_canceled ? task_group_status::canceled : task_group_status::task_complete;
}

inline void task_dynamic_state::add_notify_list(notify_list_node* notify_list) {
    if (notify_list == nullptr) return;

    notify_list_node* last_node = notify_list;

    while (last_node->next_node != nullptr) {
        last_node = last_node->next_node;
    }

    notify_list_node* current_notify_list_head = m_notify_list_head.load(std::memory_order_acquire);
    last_node->next_node = current_notify_list_head;

    while (!m_notify_list_head.compare_exchange_strong(current_notify_list_head, notify_list)) {
        __TBB_ASSERT(!represents_completed_task(current_notify_list_head) &&
                     !represents_canceled_task(current_notify_list_head) &&
                     !represents_transferred_completion(current_notify_list_head),
                     "Task receiving the completion was executed or completed");
        // Other thread updated the head of the list
        last_node->next_node = current_notify_list_head;
    }
}

// Notifies 
inline task_handle_task* task_dynamic_state::fetch_list_and_notify_all(notify_list_state_flag state_flag) {
    __TBB_ASSERT(state_flag == COMPLETED_FLAG || state_flag == CANCELED_FLAG, "Unexpected state_flag");
    notify_list_node* node = fetch_notify_list(state_flag);
    task_handle_task* next_task = nullptr;
    bool bypass_allowed = true;

    while (node != nullptr) {
        notify_list_node* next_node = node->next_node;

        // Don't dereference node after the notification!
        notify_list_node::notify_result_type result = state_flag == COMPLETED_FLAG ?
                                                      node->notify_on_completion() :
                                                      node->notify_on_cancellation();
        if (!result.second) bypass_allowed = false;
        task_handle_task* successor_task = result.first;

        if (next_task == nullptr) {
            next_task = successor_task;
        } else if (successor_task != nullptr) {
            d1::spawn(*successor_task, successor_task->ctx());
        }

        node = next_node;
    }

    if (next_task && !bypass_allowed) {
        d1::spawn(*next_task, next_task->ctx());
        next_task = nullptr;
    }
    return next_task;
}

inline task_handle_task* task_dynamic_state::complete_and_try_get_successor() {
    task_handle_task* next_task = nullptr;
    notify_list_node* node = m_notify_list_head.load(std::memory_order_acquire);

    // Doing a single check is enough since the this function is called after the task body and
    // the state of the list cannot change to transferred
    if (!represents_transferred_completion(node)) {
        next_task = fetch_list_and_notify_all(COMPLETED_FLAG);
    }
    return next_task;
}

inline task_handle_task* task_dynamic_state::cancel_and_try_get_successor() {
    __TBB_ASSERT(!represents_transferred_completion(m_notify_list_head.load(std::memory_order_relaxed)),
                 "canceled task completion cannot be transferred");
    return fetch_list_and_notify_all(CANCELED_FLAG);
}

inline void dynamic_state_task::transfer_completion_to(task_handle& receiving_task) {
    __TBB_ASSERT(receiving_task, nullptr);
    task_dynamic_state* current_state = m_dynamic_state.load(std::memory_order_relaxed);
    
    // If dynamic state was not created for currently executing task,
    // it cannot have successors or associated completion handles
    if (current_state != nullptr) {
        current_state->transfer_completion_to(task_handle_accessor::get_task_dynamic_state(receiving_task));
    }
}

class task_completion_handle {
public:
    task_completion_handle() : m_task_state(nullptr) {}

    task_completion_handle(const task_completion_handle& other) 
        : m_task_state(other.m_task_state)
    {
        // Register one more co-owner of the dynamic state
        if (m_task_state) m_task_state->reserve();
    }
    task_completion_handle(task_completion_handle&& other)
        : m_task_state(other.m_task_state)
    {
        other.m_task_state = nullptr;
    }

    task_completion_handle(const task_handle& th)
        : m_task_state(nullptr)
    {
        __TBB_ASSERT(th, "Construction of task_completion_handle from an empty task_handle");
        m_task_state = th.m_handle->get_dynamic_state();
        // Register one more co-owner of the dynamic state
        m_task_state->reserve();
    }

    ~task_completion_handle() {
        if (m_task_state) m_task_state->release();
    }

    task_completion_handle& operator=(const task_completion_handle& other) {
        if (m_task_state != other.m_task_state) {
            // Release co-ownership on the previously tracked dynamic state
            if (m_task_state) m_task_state->release();

            m_task_state = other.m_task_state;

            // Register new co-owner of the new dynamic state
            if (m_task_state) m_task_state->reserve();
        }
        return *this;
    }

    task_completion_handle& operator=(task_completion_handle&& other) {
        if (this != &other) {
            // Release co-ownership on the previously tracked dynamic state
            if (m_task_state) m_task_state->release();
    
            m_task_state = other.m_task_state;
            other.m_task_state = nullptr;
        }
        return *this;
    }

    task_completion_handle& operator=(const task_handle& th) {
        __TBB_ASSERT(th, "Assignment of task_completion_state from an empty task_handle");
        task_dynamic_state* th_state = th.m_handle->get_dynamic_state();
        __TBB_ASSERT(th_state != nullptr, "No state in the non-empty task_handle");
        if (m_task_state != th_state) {
            // Release co-ownership on the previously tracked dynamic state
            if (m_task_state) m_task_state->release();

            m_task_state = th_state;

            // Reserve co-ownership on the new dynamic state
            m_task_state->reserve();
        }
        return *this;
    }

    explicit operator bool() const noexcept { return m_task_state != nullptr; }
private:
    friend bool operator==(const task_completion_handle& t, std::nullptr_t) noexcept {
        return t.m_task_state == nullptr;
    }

    friend bool operator==(const task_completion_handle& lhs, const task_completion_handle& rhs) noexcept {
        return lhs.m_task_state == rhs.m_task_state;
    }

#if !__TBB_CPP20_COMPARISONS_PRESENT
    friend bool operator==(std::nullptr_t, const task_completion_handle& t) noexcept {
        return t == nullptr;
    }

    friend bool operator!=(const task_completion_handle& t, std::nullptr_t) noexcept {
        return !(t == nullptr);
    }

    friend bool operator!=(std::nullptr_t, const task_completion_handle& t) noexcept {
        return !(t == nullptr);
    }

    friend bool operator!=(const task_completion_handle& lhs, const task_completion_handle& rhs) noexcept {
        return !(lhs == rhs);
    }
#endif // !__TBB_CPP20_COMPARISONS_PRESENT

    friend struct task_completion_handle_accessor;

    task_dynamic_state* m_task_state;
};

struct task_completion_handle_accessor {
    static task_dynamic_state* get_task_dynamic_state(task_completion_handle& tracker) {
        return tracker.m_task_state;
    }
};
#endif

} // namespace d2
} // namespace detail
} // namespace tbb

#endif /* __TBB_task_handle_H */
