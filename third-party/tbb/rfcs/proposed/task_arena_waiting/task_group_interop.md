# Improve interoperability with task groups

## Motivation

As described in the [overarching proposal](readme.md#motivation), combined use of a `task_arena`
and a `task_group` is helpful when there is a need to execute some tasks asynchronously,
but is non-trivial to do properly. Here we propose specific APIs to make it easier.

## Proposed API

We suggest new overloads for `enqueue` which additionally take `task_group` as an argument,
and the new `task_arena::wait_for` method that also takes `task_group`.

```cpp
// Defined in header <oneapi/tbb/task_arena.h>

namespace oneapi::tbb {
    class task_arena {
    public:
        ... // public types and members of class task_arena

        // Proposed new methods
        template<typename F> void enqueue(F&& f, task_group& tg);
        task_group_status wait_for(task_group& tg);
    };

    namespace this_task_arena {
        template<typename F> void enqueue(F&& f, task_group& tg);
    }
} // namespace oneapi::tbb
```

## Design discussion

### Enqueue a function as a part of a task group

There are two existing methods to submit a task for asynchronous execution in a task arena:
```cpp
template<typename F> void task_arena::enqueue(F&& f); // (1)
void task_arena::enqueue(task_handle&& h);            // (2)
```
The `this_task_arena` namespace also has two functions with the same signatures.

The proposed new overload for `enqueue` is similar to (1) but also takes `task_group` as the second argument:
```cpp
template<typename F> void task_arena::enqueue(F&& f, task_group& tg);
```
Semantically it is equivalent to (2) which argument is created by `tg.defer(std::forward<F>(f))`.
Implementation-wise it is just a header-based wrapper over (2); a more elaborated implementation
does not appear necessary. An analogous function should also be added to the `this_task_arena` namespace.

### Wait for completion of a task_group

The new proposed method of `task_arena` takes a `task_group` argument and does not return until
all tasks in that task group are complete or cancelled:
```cpp
task_group_status task_arena::wait_for(task_group& tg);
```
Note that the scope of waiting includes not only the tasks submitted via the methods of `task_arena`
but all tasks in the task group, independent of the way they were created and added as well as of
task arenas they were submitted to.

The returned value indicates the [completion status](
https://oneapi-spec.uxlfoundation.org/specifications/oneapi/v1.4-rev-1/elements/onetbb/source/task_scheduler/task_group/task_group_status_enum)
of the task group.

The method is semantically equivalent to `execute([&tg]{ return tg.wait(); })`, and can be implemented
that way. However, a better implementation for the current code base should instead use the `wait_delegate`
class (see `oneapi/tbb/task_group.h`) and directly call the `execute` library entry point with this delegate.

There is no need to have a similar function in the `this_task_arena` namespace, as it would be
no different from calling `tg.wait()`.

### Should `execute` be extended as well?

Another method, `task_arena::execute` appear similar to `enqueue` in the sense that it also takes a callable
and executes it in the arena. Should it also interoperate with a task group, and in which way?

The purpose of `execute` is to make sure that the provided callable is executed in a certain task arena,
so that any work created by the callable is shared within the arena. To achieve that, the calling thread
attempts to join the arena; if successful, it executes the callable and returns, while if not - which means
that the arena is full with other threads - the callable is wrapped into a task and delegated to those threads,
and the calling thread blocks until the task is complete.

A reasonable interoperability semantics could be that the callable, while executed in the given arena,
also counts as a task in the given group. It would be roughly equivalent to the following code:
```cpp
// auto res = ta.execute(f, tg) could mean:
{
    auto th = tg.defer([]{}); // an empty "proxy" task for counting
    ta.execute(f);
} // th is destroyed when the thread leaves the scope
```

Note that `ta.execute([&]{ tg.run(f); })` is not suitable because it submits `f` into the arena
but does not ensure its completion, and `ta.execute([&]{ tg.run_and_wait(f); })` does not work either
because it waits for all tasks in the group, not only for `f`.

Overall, it is not obvious if adding a task group parameter to `execute` is a useful extension.

### Thoughts on work isolation

It makes sense to also consider work isolation for this API. While waiting for task group completion,
the thread can take unrelated tasks for execution, and that can potentially result in a delayed return
and in latency increase. To prevent that, tasks in the group should carry a unique tag that is
also specified for the waiting call. The `isolated_task_group` preview class provides the desired
functionality, but not the regular `task_group`.

Note that extending `this_task_arena::isolate` with a task group argument would not help. `isolate`
uses a unique isolation scope for a given callable; its purpose is to isolate the work, which the callable
produces and then waits for, from every other task, and specifically from stealing "outermost" tasks which
interfere with the callable.

We can consider the following options for providing isolation in `task_arena::wait_for(task_group&)`:
- keep the `isolated_task_group` class and support it in the proposed `task_arena` extensions;
- somehow extend the `task_group` class to optionally support work isolation (might require incompatible changes);
- add an isolation tag (automatically or on demand) only when a `task_group` is used with `task_arena`.

## Open Questions

- Is there any value in implementing this proposal first as experimental/preview API?
- Should a new overload for `execute` be added, that takes a task group argument?
- Whether/how work isolation is supported needs to be decided
