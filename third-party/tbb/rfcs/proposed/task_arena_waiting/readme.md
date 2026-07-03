# Waiting in a task_arena

## Motivation

Task arenas in oneTBB are the places for threads to share and execute tasks.
A `task_arena` instance represents a configurable execution context for parallel work.

There are two primary ways to submit work to an arena: the `execute` and `enqueue` functions.
Both take a callable object and run it in the context of the arena. The callable object
might start more parallel work in the arena by invoking a oneTBB algorithm, running a flow graph,
or submitting work into a task group.
`execute` is a blocking call: the calling thread does not return until the callable object
completes. `enqueue` is a “fire-and-forget” call: the calling thread submits the callable
object as a task and returns immediately, providing no way to synchronize with the completion
of the task.

In [the oneDPL case study of asynchronous algorithms
](https://github.com/uxlfoundation/oneDPL/tree/main/rfcs/archived/asynchronous_api_general#use-case-study)
three main usage patterns were observed, each assuming subsequent synchronization either with
the asynchronous job or with all jobs previously submitted into a work queue or device.
A task arena can be considered analogous to a device or a work queue in these patterns,
but it lacks synchronization capabilities and so cannot alone support these use cases -
for that it has to be paired with something "waitable".

### Current solution: combining with a task group

In oneTBB, asynchronous execution is supported by `task_group` and the flow graph API; both allow
submitting a job and waiting for its completion later. The flow graph is more suitable for building
and executing graphs of dependent computations, while the `task_group`, as of now, allows starting
and waiting for independent tasks. Notably, both require calling `wait`/`wait_for_all` to ensure that
the work will be done. The `task_arena::enqueue`, on the other hand, being "fire-and-forget", enforces
availability of another thread in the arena to execute the task (so-called *mandatory concurrency*).

So, a reasonable solution for the described use cases seems to combine a `task_arena` with a `task_group`.
However, it is notoriously non-trivial to do right. For example, the following "naive" attempt is subtly
incorrect:
```cpp
tbb::task_arena ta{/*args*/};
tbb::task_group tg;
ta.enqueue([&tg]{ tg.run([]{ foo(); }); });
bar();
ta.execute([&tg]{ tg.wait(); });
```
The problem is that `enqueue` submits a task that calls `tg.run` to add `[]{ foo(); }` to the task group,
but it is unknown if that task was actually executed prior to `tg.wait`. Simply put,
the task group might yet be empty, in which case `tg.wait` exits prematurely.

To avoid that, `execute` can be used instead of `enqueue`, but then the mentioned
thread availability guarantee is lost. The approach with `execute` is shown in the
[oneTBB Developer Guide](https://oneapi-src.github.io/oneTBB/main/tbb_userguide/Guiding_Task_Scheduler_Execution.html)
as an example to split the work across several NUMA domains. The example utilizes the fork-join
synchronization pattern described in the mentioned oneDPL document to ensure that the work is complete
in all the arenas. It also illustrates that the problem stated in this proposal is relevant.

A better way of using these classes together, however, is the following:
```cpp
tbb::task_arena ta{/*args*/};
tbb::task_group tg;
ta.enqueue(tg.defer([]{ foo(); }));
bar();
ta.execute([&tg]{ tg.wait(); });
```
In this case, the task group "registers" a deferred task to run `foo()`, which is then enqueued
to the task arena. The task is added by the calling thread, so we can be sure that `tg.wait` will not
return until the task completes.

While combining `task_arena` with `task_group` is a viable solution, it would be good to avoid
the extra complexity and verbosity existing now.

### Waiting for all tasks in the arena

The idea to wait for completion of all tasks and parallel jobs executed in a task arena was suggested
and considered in the past for TBB. At that time, it was rejected. There was no good way to wait
for completion of all tasks in the arena: even if all the task queues are empty, for as long as any thread
still executes a task, new tasks might be produced. Essentially, waiting for arena to drain
would mean waiting for all threads to leave the arena. Also such an API was considered unsafe,
as it would result in a deadlock if accidentally called from a thread that holds a slot in the arena.

Still, supporting the "synchronize with the queue/device" pattern would in general be useful.

### Participation of application threads

A related feature considered before is *moonlighting*, that is, temporary participation of application threads
in a task arena. The idea is that if some application threads have nothing else to do, they might help with tasks
submitted by other threads.

The major questions regarding moonlighting are:
- When should a moonlighting thread return? Should that be after completion of a specific task (a task group,
  a flow graph)? Or after execution of a certain number of any tasks? Or maybe per some external signal or
  condition, such as timeout?
- Tasks that a moonlighting thread takes might contain a nested blocking call, which can prevent the thread to return
  for an unpredictable time. That is, in the presence of nested parallelism it is hard to guarantee that the thread
  will not get trapped for long and can stop moonlighting (or check exit conditions) after processing each single task.

An alternative approach could be for application threads to "yield" CPUs to TBB threads, which would execute tasks
and make progress. Even if a TBB thread is blocked in a nested wait, the application thread is not blocked and can
return. Temporary CPU oversubscription is possible, but it is likely acceptable as some other TBB thread can leave
the arena and restore utilization balance.

One more option is to use a mechanism similar to task suspension, switching the moonlighting thread to an execution
context with a separate stack which it can abandon if necessary.

## Proposal

The proposal consists of ideas that are not mutually exclusive and can be implemented independently.

### 1. Simplify the use of a task group

To address the existing shortcomings of the `task_arena` and `task_group` combination, the `enqueue` method
could take `task_group` as the second argument, and a new method could wait for a task group:
```cpp
ta.enqueue([]{ foo(); }, tg); // corresponds to: ta.enqueue(tg.defer([]{ foo(); }));
ta.wait_for(tg);              // corresponds to: ta.execute([&tg]{ tg.wait(); });
```

This part of the proposal [is supported](../../supported/task_arena_waiting/readme.md) since oneTBB 2022.3.

### 2. Reconsider waiting for all tasks

With the redesign of the task scheduler in oneTBB, we might evaluate if there is now a reliable way
to ensure completion of all jobs in an arena. The safety concern can potentially be mitigated
by throwing an exception or returning `false`, similarly to the `tbb::finalize` function.

The corresponding API could look like:
```cpp
ta.wait_for_all(); // throws tbb::unsafe_wait if called 
bool success = ta.wait_for_all(std::nothrow{}); // in case exceptions are undesirable
```
Another option for the non-throwing variation is `bool try_wait_for_all()`. With that name, it might also
have a weaker semantics omitting the guarantee of all arena work being completed.

Implementation-wise, a waiting context (reference counter) could be added to the internal arena class.
It would be incremented on each call to `execute`, `enqueue`, and `isolate`, and decremented after
the corresponding task is complete. The more tricky part is to track parallel jobs started in an
implicit thread-associated arenas; perhaps it can be done when registering and deregistering
the corresponding task group contexts. For the weaker "try" semantics, it would be OK to return
when no task to execute is found, even if some tasks are yet running; that can be done by
inspecting the arena, without adding more reference counting.

The implementation can likely be backward compatible, as no layout or function changes in `task_arena`
seems necessary. A new library entry point would be added for the waiting function.

The implementation should, if possible, assign the waiting thread to execute tasks; if not, the thread
should *block with progress delegation*, as described below.

### 3. Consider API for progress delegation

Of the outlined options for [application thread participation](#participation-of-application-threads),
exchanging an application thread for a TBB thread in the arena, or *progress delegation*, seems the least risky.
At minimum, it would enforce mandatory concurrency similarly to `enqueue`; it might also increment
the total demand for TBB worker threads, rebalance threads between active arenas, and in a more sophisticated
case could even ensure that a certain arena gets a temporary boost during rebalancing.

Since the scenario of participation or delegation with no explicit exit condition is covered by `wait_for_all`
in (2), any special API function should explicitly take an exit condition. If that condition is the completion
of specific tasks, the `wait_for` function described in (1) is likely most appropriate; besides a `task_group`,
it could also accept a `task_handle` and a `flow::graph`, with the implementation equivalent to
`ta.execute([&]{ argument_specific_wait(); })`.

So any actual progress delegation function would take an argument for an external exit condition.
The most universal approach is seemingly to take a callable that blocks the executing thread,
and run that callable after "summoning" a worker thread to the arena. The API sketch for that is:
```cpp
ta.block_with_progress_delegation([]{ std::this_thread::sleep_for(100ms); });
```

## Open Questions

- Implementation feasibility for (2) and (3) needs to be explored
- API names and semantic details need to be further elaborated
- Whether/how work isolation is supported needs to be decided
