# Waiting for Individual Tasks in ``task_group``

This proposal introduces an extension for the oneTBB ``task_group`` and ``task_arena`` APIs, enabling the ability to wait for the completion of
individual tasks.

## Table Of Contents

* 1 [Introduction](#introduction)
* 2 [Proposal](#proposal)
* 2.1 [Returning the Status of the Task](#returning-the-status-of-the-task)
* 2.2 [Feature Test Macro](#feature-test-macro)
* 2.3 [Summary](#summary)
* 2.3.1 [Member functions of ``task_group`` class](#member-functions-of-task_group-class)
* 2.3.2 [Member functions of ``task_arena`` class](#member-functions-of-task_arena-class)
* 3 [Future Enhancements](#future-enhancements)
* 3.1 [Run-and-wait methods for ``task_arena``](#run-and-wait-methods-for-task_arena)
* 3.2 [Work Isolation Issue](#work-isolation-issue)
* 4 [Exit Criteria and Open Questions](#exit-criteria-and-open-questions)
* 5 [Implementation Details](#implementation-details)
* 5.1 [Avoid Bypassing the Successor](#avoid-bypassing-the-successor)
* 6 [Alternative Implementation Approaches](#alternative-implementation-approaches)
* 6.1 [Assign Single ``wait_context`` to the Task](#assign-single-wait_context-to-the-task)
* 6.2 [Avoid ``wait_context`` Usage](#avoid-wait_context-usage)

## Introduction
The ``tbb::task_group`` class represents a collection of tasks that execute concurrently. Tasks can be dynamically added to the group during execution.

The [Task Group Dynamic Dependencies preview feature](../task_group_dynamic_dependencies/README.md) extends the ``task_group`` API to support task dependencies, ensuring that a *successor* task begins execution only after all its *predecessors* have completed.

The current ``task_group`` API allows waiting for the completion of all tasks in the group using ``task_group::wait`` or ``task_group::run_and_wait``.
The waiting thread may participate in executing other tasks within the ``task_arena``, which may not be related to the specific ``task_group``.

Although waiting for all tasks suits many use cases, some workflows would benefit from the ability to wait for individual tasks.

Consider an example using an out-of-order SYCL queue.

```cpp
sycl::queue q{};

// Creating multiple tasks for the queue
sycl::event task1 = q.single_task(task1_body);
sycl::event task2 = q.memset(some_memory_ptr, 0, 1024);

sycl::event task3 = q.submit([&](sycl::handler& h) {
    h.depends_on(std::vector<sycl::event>{task1, task2});

    // task1_body executes
    // some_memory_ptr is set before task3_body runs
    h.single_task(task3_body);
});

task3.wait(); // wait a single task

// Submit additional work
q.wait();
```

Consider implementing a similar model using ``tbb::task_group``. The queue can be represented as a ``task_group`` object, where tasks correspond to command groups in the queue, and events are modeled using ``tbb::task_completion_handle``s.

```cpp
using event = tbb::task_completion_handle;

class queue {
    template <typename Body>
    event single_task(Body body) {
        tbb::task_handle task = m_task_group.defer(body);
        tbb::task_completion_handle comp_handle = task;
        m_task_group.run(std::move(task));
        return comp_handle;
    }

    void wait() { m_task_group.wait(); }
private:
    tbb::task_group m_task_group;
};
```

The ``sycl::handler::depends_on`` functionality can be implemented using the ``tbb::task_group::set_task_order`` API:

```cpp
class handler {
    void depends_on(const std::vector<event>& events) {
        for (event& e : events) {
            tbb::task_group::set_task_order(e, m_root);
        }
    }

    tbb::task_handle m_root;
};
```

Since the example above also uses ``sycl::event::wait`` to wait for the completion of a single task (``task3``), implementing this behavior with
a ``task_group`` requires support for waiting on a single ``task_completion_handle`` object.

## Proposal

The proposal aims to extend ``tbb::task_group`` and ``tbb::task_arena`` APIs by introducing new waiting functions that accept a
``task_completion_handle`` argument.
These functions exit when the task represented by the handle argument is completed or when the group execution is cancelled.
Other tasks within the ``task_group`` may not be complete when these new waiting function return.

To simplify the submission and waiting process for a single task, it is also proposed to introduce submit-and-wait functions that accept a
``task_handle`` argument.

If task completion is transferred to another task using ``tbb::task_group::transfer_completion_to(other_task)``, the waiting functions will wait for
the completion of ``other_task``.

The current waiting functions return a ``task_group_status``, which can have three possible values:
* ``tbb::task_group_status::not_complete`` - The group execution is not cancelled, and not all tasks in the group have completed.
* ``tbb::task_group_status::complete`` - The group execution is not cancelled, and all tasks in the group have completed.
* ``tbb::task_group_status::canceled`` - The group execution is cancelled, and the completion status of tasks is unknown.

Returning the same set of statuses from the new waiting functions is not sufficient.

If the group execution is cancelled, the tracked task may still execute, and returning ``canceled`` does not accurately reflect the task's
completion status.
If the execution is not cancelled, the function would need to track whether other tasks remain in the group and return ``not_complete`` if any are still pending.

To address this and to make new waiting functions consistent in the return type with other functions in ``task_group`` and ``task_arena``,
it is proposed to introduce a new status ``task_complete`` that indicates that the awaited task is completed.
It is unknown either the ``task_group`` is cancelled or not, since the task can still be completed even if the group execution was cancelled.

If the task group execution is canceled and the originally executed task has transferred its completion to another task that was canceled, the
waiting function will return ``task_group_status::canceled``.

### Returning the Status of the Task

It may be beneficial to execute the waiting function only if the awaited task has not yet completed or
to execute unrelated work in the main thread while periodically checking to see if the awaited task is complete.

```cpp
task_completion_handle ch = task;
if (!is_completed(ch)) {
    tg.wait_for_task(ch);
}
```

or 

```cpp
task_completion_handle ch = task;
while (!is_completed(ch)) {
    do_some_other_work();
}
```

To determine the status of a task represented by ``task_completion_handle``, it is proposed to add a new function ``task_group::get_status_of(task_completion_handle&)``.
This function returns:
* ``task_group_status::not_complete`` - if the task has not yet been submitted, is being scheduled, or is currently executing.
* ``task_group_status::task_complete`` - if the task has finished executing successfully.
* ``task_group_status::canceled`` - if the task was not executed due to task group cancellation.

If the completion of the task originally represented by the ``task_completion_handle`` argument has been transferred to another task, the function returns the status of the task that
received the completion.

The ``get_status_of`` function can also be implemented as a member function of the ``task_completion_handle`` class.

An alternative is having two separate functions: ``is_completed`` and ``is_canceled``. The advantage of this approach is that ``task_group_status`` does not need to be used. However,
the example above would require two function calls:

```cpp
task_completion_handle ch = task;
if (!is_completed(ch) && !is_canceled(ch)) {
    tg.wait_for_task(ch);
}
```

Both ``is_completed`` and ``is_canceled`` can also be implemented as member functions of the ``task_completion_handle`` class.

### Feature Test Macro

It is proposed to add an explicit feature-test macro to determine the presence of the feature:

```cpp
#define TBB_PREVIEW_TASK_GROUP_EXTENSIONS 1
#include <oneapi/tbb/task_group.h>

#if TBB_HAS_TASK_GROUP_WAIT_FOR_SINGLE_TASK >= 202xxx // Some documented value
group.wait_for_task(comp_handle);
#else
// Usage of other APIs or self-written workarounds
#endif
```

An alternative is to use ``TBB_VERSION`` macro for this purpose:

```cpp
#if TBB_VERSION >= VER // VER is the first TBB version where waiting for individual tasks was released
```

### Summary

```cpp
// Defined in <oneapi/tbb/task_group.h>
// Defined in <oneapi/tbb/version.h>
#define TBB_HAS_TASK_GROUP_WAIT_FOR_SINGLE_TASK 202xxx

namespace oneapi {
namespace tbb {

// Defined in <oneapi/tbb/task_group.h>
enum task_group_status { // Existing API
    not_complete,        // Existing API
    complete,            // Existing API
    canceled,            // Existing API
    task_complete        // Proposed API 
}

class task_group {
    task_group_status wait_for_task(task_completion_handle& comp_handle);
    task_group_status run_and_wait_for_task(task_handle&& handle);

    task_group_status get_status_of(task_completion_handle& comp_handle);
};

// Defined in <oneapi/tbb/task_arena.h>
class task_arena {
    task_group_status wait_for(task_completion_handle& comp_handle);
};

} // namespace tbb
} // namespace oneapi
```

#### Member functions of ``task_group`` class

```cpp
task_status wait_for_task(task_completion_handle& comp_handle);
```

Waits for the completion of the task represented by ``comp_handle``.
If completion was transferred to another task using ``tbb::task_group::transfer_completion_to``, the function waits for the completion of that task.

Returns ``task_status::completed`` if the task was executed, or ``task_status::canceled`` if it was not.

```cpp
task_status run_and_wait_for_task(task_handle&& handle);
```

Schedules the task represented by ``handle`` for execution (if it has no unresolved dependencies), and waits for its completion.

This is semantically equivalent to: ``task_completion_handle comp_handle = handle; run(std::move(handle)); wait_for_task(comp_handle)``.

Returns ``task_status::completed`` if the task was executed, or ``task_status::canceled`` if it was not.

```cpp
task_group_status get_status_of(task_completion_handle& comp_handle);
```

Returns the status of the task represented by ``comp_handle``:
* ``task_group_status::not_complete`` - if the task has not been submitted for execution, or has not finished execution.
* ``task_group_status::task_complete`` - if the task has finished execution.
* ``task_group_status::canceled`` - if the task execution was cancelled.

If the completion was transferred to another task using ``tbb::task_group::transfer_this_task_completion_to``, the function returns the status of that task.

#### Member functions of ``task_arena`` class

```cpp
task_status wait_for(task_completion_handle& comp_handle);
```

Waits for the completion of the task represented by ``comp_handle``.
If completion was transferred to another task using ``tbb::task_group::transfer_completion_to``, the function waits for completion of that task.

This is semantically equivalent to: ``execute([&] { tg.wait_for_task(comp_handle); })``, where ``tg`` is a ``task_group`` where the task referred by ``comp_handle`` is registered.

Returns: ``task_status::completed`` if the task was executed and ``task_status::canceled`` otherwise.

Introducing ``wait_for`` in the ``this_task_arena`` namespace is unnecessary, as it would be functionally identical to calling the ``task_group::wait_for_task`` API directly.

## Future Enhancements

### Run-and-wait methods for ``task_arena``

It may be beneficial to introduce APIs that simplify task submission into ``task_arena`` and support waiting for task completion.

This functionality would be semantically similar to:

```cpp
tbb::task_group tg;
tbb::task_arena arena;
tbb::task_handle task = tg.defer(...);

tbb::task_completion_handle comp_handle = task;
arena.enqueue(std::move(task));
task_status status = arena.wait_for(comp_handle);
```

One possible name for such an extension is ``task_arena::enqueue_and_wait_for(task_handle&& task)``.
This function is semantically close to ``task_arena::execute`` (which may submit the task and wait for its completion), but it guarantees that
the task is enqueued into the arena.

A similar function could also be added to the ``tbb::this_task_arena`` namespace.

### Work Isolation Issue

Similar to other ``task_arena::wait_for`` overload that take a ``task_group`` argument, it makes sense to also consider work isolation for
the APIs introduced in this proposal.
If the wait is unrestricted, the waiting thread may execute any task from the arena, including those unrelated to the completion of the awaited
task or its associated ``task_group``.

Ideally, the waiting thread should only execute tasks that contribute to the completion of the awaited task. In such a model, all tasks
within the same subgraph would need to share a common isolation tag. In theory, a successor could inherit the isolation tag from
its predecessor. However, multiple predecessors may have different tags.

This would require changes to the current isolation mechanism to support multiple isolation tags, allowing a thread waiting for a successor
to execute tasks with any of its predecessor's tags. 

As an initial step, it makes sense to isolate the waiting thread so that it only executes tasks related to the same ``task_group`` as the awaited task - similar to the improvement described in the [RFC for another overload of ``task_arena::wait_for``](../../supported/task_arena_waiting/task_group_interop.md).

### ``run_and_wait_for_task`` Accepting the Task Body

The following API extension for ``task_group`` may also be beneficial to avoid unnecessary ``task_handle`` creation before the wait:

```cpp
class task_group {
    template <typename Func>
    task_status run_and_wait_for_task(const Func& f);
};
```

This API is semantically equivalent to ``return run_and_wait_for_task(defer(f));``. 

Since, in this API, the ``task_handle`` owning the task is not accessible by user, the awaited task cannot have any dependencies and is likely to be executed by the calling thread.
If ``f`` does not invoke ``transfer_this_task_completion_to``, usage of the function makes no sense since the thread can just call ``f()`` directly. 

But when ``f`` does invoke ``transfer_this_task_completion_to(other_task)``, the function would wait for ``other_task`` (that may have dependencies) to complete.

Consider the following example:

```cpp
// Run the root divide-and-conquer task
tg.run_and_wait_for_task([&tg] {
    auto left_leaf = tg.defer(...);
    auto right_leaf = tg.defer(...);
    auto join = tg.defer(...);

    tbb::task_group::set_task_order(left_leaf, join);
    tbb::task_group::set_task_order(right_leaf, join);
    tbb::task_group::transfer_this_task_completion_to(join);

    tg.run(std::move(left_leaf));
    tg.run(std::move(right_leaf));
    tg.run(std::move(join));
});
```

``run_and_wait_for_task`` will exit only after the ``join`` task completes. If several divide-and-conquer tasks are executed in the same ``task_group``, such a function can
be used to wait for the completion of each individual task.

## Exit Criteria and Open Questions

The following questions should be resolved before promoting the feature out of the ``experimental`` stage.

* Performance targets for this feature should be clearly defined and met.
* Should the ``enqueue_and_wait`` API be added to ``task_arena``? Refer to the
  [Run-and-wait methods for ``task_arena`` section](#run-and-wait-methods-for-task_arena) for more details.
* Should work isolation constraints be applied while waiting for a task to complete? See the
  [Work Isolation Issue section](#work-isolation-issue) for more details.
* Should ``run_and_wait_for_task`` overload accepting the task body be added? Refer to the
  [``run_and_wait_for_task`` Accepting the Task Body](#run_and_wait_for_task-accepting-the-task-body) for more details.
* Proper API and naming for functions returning the task status should be defined. Refer to the
  [``Returning the status of the task``](#returning-the-status-of-the-task) section for more details.
* Should an additional ``task_group_status`` be introduced to indicate that the task has been completed, but a cancellation of the group was detected?

## Implementation Details

To implement the proposed API, the successors list used in [Task Group Dynamic Dependencies](../task_group_dynamic_dependencies/implementation_details.md) can be reused.

The list can be modified to support two types of nodes representing entities that require notification upon task completion: successor nodes
and waiter nodes.
A successor node is an existing construct used to establish dependencies between tasks.
A waiter node represents a ``wait_context`` object with a reference count of ``1``, assigned to each ``wait_for_task`` call.

Each ``wait_for_task`` call creates a new waiter node and adds it to the notification list similarly to adding the successor node.

When a task completes, it traverses its notification list as follows:
* For each successor node, the dependency counter is decremented.
* For each waiter node, the reference counter in the associated ``wait_context`` is decremented, notifying the waiting thread of task completion.

The diagram below illustrates a notification list for a task with two successors and two ``wait_for_task`` calls awaiting its completion:

<img src="notify_list_before_completion.png" width=800>

To differentiate between completed and canceled tasks and return the correct ``task_status``, distinct signals are sent while traversing the
notification list after ``task::execute`` and ``task::cancel``.

### Avoid Bypassing the Successor

Consider an example where a sequence of three tasks is implemented using ``task_group`` and waiting for the middle task:

```cpp
tbb::task_group tg;

tbb::task_handle begin_task = tg.defer(start_body);
tbb::task_handle middle_task = tg.defer(middle_body);
tbb::task_handle end_task = tg.defer(end_body);

tbb::task_group::set_task_order(begin_task, middle_task);
tbb::task_group::set_task_order(middle_task, end_task);

tg.run(std::move(begin_task));
tg.run(std::move(end_task));
tg.run_and_wait_for_task(std::move(middle_task));

// Post middle task actions

tg.wait();
```

The internal task graph is illustrated on the diagram below. ``SN`` and ``WN`` refer to the successor and waiter nodes in the notification list,
respectively.

<img src="avoid_bypass_graph.png" width=400>

For simplicity, assume that the task graph is executed within a single-threaded arena.

In the current implementation of Task Group Dynamic Dependencies, when a task completes and notifies its list,
one of the next ready-to-execute tasks (with zero dependencies) is bypassed.

When the calling thread invokes ``run_and_wait_for_task`` call, the ``begin_task`` is executed. After executing its body,
it notifies its single successor node in the list. The ``middle_task``'s dependency count becomes zero, and the task is bypassed.

When ``middle_task`` completes, it notifies its list, which contains both a successor node and a waiter node. The successor node notifies
``end_task``, making it available for execution. The waiter node decrements the reference count in the associated ``wait_context``, allowing the
corresponding ``run_and_wait_for_task`` call to complete.
The notified ``wait_context`` is the same context awaited by the calling thread.

If ``end_task`` is bypassed, the calling thread will not exit until all tasks in the graph have completed. If additional tasks follow ``end_task``,
the thread will only exit after each is bypassed sequentially.

However, since the current thread notifies the ``wait_context`` it is waiting on via the corresponding waiter node, it is more appropriate to avoid
bypassing the next task. Instead, the task should be spawned, and ``run_and_wait_for_task`` should exit after executing ``middle_task``.

For the initial implementation, it is proposed to completely avoid bypassing the task returned from the notification list and to spawn it instead.

There are several ways to improve the implementation in the future.

One approach is to store a pointer to the innermost ``wait_context`` that the current thread is waiting on, within the task dispatcher associated
with the calling thread.

When the wait node is notified, the thread checks whether the context pointer in the node matches the one the current thread is waiting on and avoids
bypassing if they match.

Implementing the check requires introducing a new entry point to access the ``wait_context`` stored in the task dispatcher.

While this approach avoids task bypassing when the waiting thread executes the task it is explicitly waiting for, bypassing remains
unrestricted in other scenarios. For example, if the waiting thread executes a task unrelated to the awaited one, such as a ``parallel_for``
task in the same arena, it may still enter the bypass loop, even if the reference count of the awaited ``wait_context`` is zero.

An alternative approach is to implement a general mechanism within the scheduler that forces the thread to exit the bypass loop and spawn the returned task
if further execution should not be continued (i.e., ``waiter.continue_execution()`` returns ``false``).

## Alternative Implementation Approaches

### Assign Single ``wait_context`` to the Task

The first alternative approach is to assign a single ``wait_context`` to each task and reuse it for all waiters. The ``wait_context`` would become a field within the ``task_dynamic_state`` class:

<img src="wait_context_part_of_state.png" width=800>

The advantage of this approach compared to the recommended one is that a single ``wait_context`` object can be reused across all ``wait_for_task`` calls waiting on the same task.

On the other hand, this approach creates a ``wait_context`` object for every task, including those not awaited by any ``wait_for_task`` calls. 
In contrast, the recommended approach creates a context object only when a ``wait_for_task`` call is made.

Another disadvantage of this approach is that the ``wait_context`` the thread is waiting on must be updated if completion is transferred to another task:

```cpp
// wait_for_task(comp_handle) implementation
task_dynamic_state* state = comp_handle.get_dynamic_state();

r1::wait(state->get_wait_context());

while (state->was_transferred()) {
    state = state->transfer_state();
    r1::wait(state->get_wait_context());
}
```

### Avoid ``wait_context`` Usage

The second alternative is to avoid using ``wait_context`` entirely, as it would hold only a single reference released upon task completion.
This requires the introduction of the generalized wait mechanism in the scheduler, allowing threads to wait on a predicate rather than 
a context object:

```cpp
struct wait_interface {
    virtual bool continue_execution() = 0;
    virtual std::uintptr_t wait_id() = 0;
};

void wait(wait_context&, task_group_context&); // existing entry point
void wait(wait_interface&, task_group_context&); // new entry point for generalized wait
```

``wait_interface::continue_execution`` acts as a predicate. When it returns ``false``, the waiting thread exits the scheduler, completing the 
corresponding ``wait_for_task`` call.

``wait_interface::wait_id`` should return a unique identifier for the wait, enabling notification of waiting threads via the concurrent monitor
in the TBB scheduler.

The existing entry point can be implemented using ``wait_interface``:

```cpp
// Existing class, fields and functions
class wait_context {
    std::atomic<std::uint64_t> m_ref_count;

    bool continue_execution() const {
        std::uint64_t r = m_ref_count.load;
        return r > 0;
    }
};

struct wait_on_context : wait_interface {
    wait_context& m_wait_context;

    bool continue_execution() override {
        return m_wait_context.continue_execution();
    }

    std::uint64_t wait_id() override {
        return std::uintptr_t(&m_wait_context);
    }
};

void wait(wait_interface&, task_group_context& ctx);

void wait(wait_context& context, task_group_context& ctx) {
    wait_on_context waiter(context);
    wait(waiter, ctx);
}
```

Waiting for a task to complete can be implemented as shown below:

```cpp
// Existing class, layout unchanged
class task_dynamic_state {
    task_handle_task*        m_task;
    successor_list_node*     m_successor_list_head;
    task_dynamic_state*      m_new_completion_point;
    std::atomic<std::size_t> m_num_dependencies;
    std::atomic<std::size_t> m_num_references;
    small_object_allocator   m_allocator;
};

struct wait_on_task : wait_interface {
    task_dynamic_state* m_task_state;

    bool continue_execution() override {
        return !m_task_state->represents_completed_task(); // reads m_successor_list_head state
    }

    std::uintptr_t wait_id() override {
        return std::uintptr_t(m_task_state);
    }
};

void wait_for_task(task_completion_handle& comp_handle) {
    task_dynamic_state* state = comp_handle.get_dynamic_state();

    wait_on_task task_waiter(state);

    r1::wait(task_waiter);

    while (state->was_transferred()) {
        state = state->transfer_state();
        wait_on_task transfer_waiter(state);

        r1::wait(transfer_waiter);
    }
}
```

One advantage of this approach over the recommended one is that it eliminates the need to instantiate a ``wait_context`` object, enabling the scheduler to
directly access the awaited task's state.

The primary disadvantage - similar to the [first alternative](#assign-single-wait_context-to-the-task) - is that a single ``r1::wait`` call is insufficient to handle completion
transfer. This limitation arises because the ``wait_id`` must be updated to ensure accurate thread notification when the completion of the task has been transferred.
