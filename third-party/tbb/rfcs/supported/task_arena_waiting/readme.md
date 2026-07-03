# Waiting in a task_arena

For more details on waiting for work in a task arena, see
[the corresponding RFC proposal](../../proposed/task_arena_waiting/readme.md).
This document covers parts that have been implemented in oneTBB.

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

Therefore, there was no convenient way to submit work for asynchronous execution **and** later wait
for completion of that work.

### Earlier solution: combining with a task group

In oneTBB, asynchronous execution is supported by `task_group` and the flow graph API; both allow
submitting a job and waiting for its completion later.
Notably, both require calling `wait`/`wait_for_all` to ensure that
the work will be done. The `task_arena::enqueue`, on the other hand, being "fire-and-forget", enforces
availability of another thread in the arena to execute the task (so-called *mandatory concurrency*).

So, a reasonable solution for the described use cases seems to combine a `task_arena` with a `task_group`.
However, it was notoriously non-trivial to do right. For example, the following "naive" attempt is subtly
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
synchronization pattern to ensure that the work is complete
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

## Implemented improvements

To address extra complexity and verbosity of using together `task_arena` and `task_group`, the `enqueue` method
of `task_arena` is overloaded to take `task_group` as the second argument, and a new method is added to wait
for a task group:
```cpp
ta.enqueue([]{ foo(); }, tg); // corresponds to: ta.enqueue(tg.defer([]{ foo(); }));
ta.wait_for(tg);              // corresponds to: ta.execute([&tg]{ tg.wait(); });
```

This API has been implemented since oneTBB 2022.3.
See [Improve interoperability with task groups](task_group_interop.md) for more details.

The example code to split work across NUMA-bound task arenas can now look like this (assuming also
a special function that creates and initializes a vector of arenas):
```cpp
std::vector<tbb::task_arena> numa_arenas =
    initialize_constrained_arenas(/*some arguments*/);
std::vector<tbb::task_group> task_groups(numa_arenas.size());

for(unsigned j = 0; j < numa_arenas.size(); j++) {
    numa_arenas[j].enqueue( (){/*some parallel stuff*/}, task_groups[j] );
}

for(unsigned j = 0; j < numa_arenas.size(); j++) {
    numa_arenas[j].wait_for( task_groups[j] );
}
```
