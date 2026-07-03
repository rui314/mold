# API to simplify creation of task arenas constrained to NUMA nodes

This sub-RFC proposes an API to ease creation of a one-per-NUMA-node set of task arenas.

## Introduction

The code example in the [overarching RFC for NUMA support](README.md) shows the likely
pattern of using task arenas to distribute computation across all NUMA domains on a system.
Let's take a closer look at the part where arenas are created and initialized.

```c++
  std::vector<tbb::numa_node_id> numa_nodes = tbb::info::numa_nodes();
  std::vector<tbb::task_arena> arenas(numa_nodes.size());
  std::vector<tbb::task_group> task_groups(numa_nodes.size());

  // initialize each arena, each constrained to a different NUMA node
  for (int i = 0; i < numa_nodes.size(); i++)
    arenas[i].initialize(tbb::task_arena::constraints(numa_nodes[i]), 0);
```

The first line obtains a vector of NUMA node IDs for the system. Then, a vector of the same size
is created to store `tbb::task_arena` objects, each constrained to one of the NUMA nodes.
Another vector holds `task_group` instances used later to submit and wait for completion
of the work in each of the arenas - it is necessary because `task_arena` does not provide
any work synchronization API. Finally, the loop over all NUMA nodes initializes associated
task arenas with proper constraints.

While not incomprehensible, the code is quite verbose and arguably too explicit for the typical scenario
of creating a set of arenas across all available NUMA domains. There is also risk of subtle issues.
The default constructor of `task_arena` reserves a slot for an application thread. The arena initialization
at the last line explicitly overwrites it to 0 to allow TBB worker threads taking all the slots, however
this nuance might be unknown and easy to miss, potentially resulting in underutilization of CPU resources.

## Proposal

We propose to introduce a special function to create the set of task arenas, one per NUMA node on the system.
The initialization code equivalent to the example above would be:

```c++
  std::vector<tbb::task_arena> arenas = tbb::create_numa_task_arenas();
  std::vector<tbb::task_group> task_groups(arenas.size());
```

The rest of the code in that example might be rewritten with the API proposed in
[Waiting in a task arena](../task_arena_waiting/readme.md):

```c++
  // enqueue work to all but the first arena, using the task groups to track work
  for (int i = 1; i < arenas.size(); i++)
    arenas[i].enqueue(
      [] { tbb::parallel_for(0, N, [](int j) { f(w); }); }, 
      task_groups[i]
    );

  // directly execute the work to completion in the remaining arena
  arenas[0].execute([] {
    tbb::parallel_for(0, N, [](int j) { f(w); });
  });

  // join the other arenas to wait on their task groups
  for (int i = 1; i < arenas.size(); i++)
    arenas[i].wait_for(task_groups[i]);
```

### Public API

The function has the following signature:

```c++
// Defined in tbb/task_arena.h

namespace tbb {
    std::vector<tbb::task_arena> create_numa_task_arenas(
        task_arena::constraints other_constraints = {},
        unsigned reserved_slots = 0
    };
}
```

It optionally takes a `constraints` argument to change default arena settings such as maximal concurrency
(the upper limit on the number of threads), core type etc.; the `numa_id` value in `other_constraints`
is ignored. The second optional argument allows to override the number of reserved slots, which by default
is 0 (unlike the `task_arena` construction default of 1) for the reasons described in the introduction.

These arena parameters were selected for pre-setting because there appear to be practical use cases to modify
it uniformly for the whole arena set - e.g., to suppress the use of hyper-threading or to reserve a slot
for a dedicated application thread. For other arena parameters, such as priorities and thread leave policy,
no obvious use cases are seen for uniformly changing default values; it can be addressed on demand.

The function returns a `std::vector` of created arenas. The arenas should not be initialized,
in order to allow changing certain arena settings before the use.

### Possible implementation

```c++
std::vector<tbb::task_arena> create_numa_task_arenas(
    tbb::task_arena::constraints other_constraints,
    unsigned reserved_slots)
{
    std::vector<tbb::numa_node_id> numa_nodes = tbb::info::numa_nodes();
    std::vector<tbb::task_arena> arenas;
    arenas.reserve(numa_nodes.size());
    for (tbb::numa_node_id nid : numa_nodes) {
        other_constraints.numa_id = nid;
        arenas.emplace_back(other_constraints, reserved_slots);
    }
    return arenas;
}
```

### Shortcomings and downsides

The following critics was provided in the RFC discussion:

- It might be confusing that a single `constraints` object is used to generate multiple arenas,
  especially with part of it (`numa_id`) being ignored.
- The proposed API addresses just one, albeit important, use case for creating a set of arenas.

See the "universal function" alternative below for related considerations.

## Considered alternatives

### Sub-classing `task_arena`

The earlier proposal [PR #1559](https://github.com/uxlfoundation/oneTBB/pull/1559) also aimed to simplify
the typical usage pattern of NUMA arenas, with possibility to extend to other similar cases.

It suggested to add a new class derived from `task_arena` which would have "only necessary methods
to allow submission and waiting of a parallel work", by selectively exposing methods of `task_arena`
and also adding a method to wait for work completion. Instances of such class could only be created
via a factory function that would instantiate a ready-to-use arena for each of the NUMA domains.

```c++
class constrained_task_arena : protected task_arena {
public:
    using task_arena::is_active;
    using task_arena::terminate;
    using task_arena::max_concurrency;

    using task_arena::enqueue;
    using task_arena::execute; // not in the original proposal

    void wait();
    friend std::vector<constrained_task_arena> initialize_numa_constrained_arenas();
};
```

In the code example used in this document, that API (with the method `execute` also exposed)
would fully eliminate explicit use of `task_group`:

```c++
  std::vector<tbb::constrained_task_arena> arenas =
    tbb::initialize_numa_constrained_arenas();

  // enqueue work to all but the first arena
  for (int i = 1; i < arenas.size(); i++)
    arenas[i].enqueue([] {
      tbb::parallel_for(0, N, [](int j) { f(w); });
    });

  // directly execute the work to completion in the remaining arena
  arenas[0].execute([] {
    tbb::parallel_for(0, N, [](int j) { f(w); });
  });

  // join the other arenas to wait on their task groups
  for (int i = 1; i < arenas.size(); i++)
    arenas[i].wait();
```

While suggesting more concise and error-protected API for the problem at question, that approach has
its downsides:
- Adding a special "flavour" of `task_arena` potentially increases the library learning curve and
  might create confusion about which class to use in which conditions, and how these interoperate.
- It seems very specialized, capable to only address specific and quite narrow set of use cases.
- Arenas are pre-initialized and so could not be adjusted after creation.

Our proposal, instead, aims at a single usability aspect with incremental improvements/extensions
to the existing oneTBB classes and usage patterns, leaving other aspects to complementary proposals.
Specifically, the [Waiting in a task arena](../task_arena_waiting/readme.md) RFC improves the joint
use of a task arena and a task group to submit and wait for work. Combined, these extensions will
provide only a slightly more verbose solution for the NUMA use case, while being more flexible
and having greater potential for other useful extensions and applications.

### Universal function to create an arena set

In the discussion of this RFC, it was suggested to generalize the function for creating a set of arenas
with various prescribed characteristics, beyond only NUMA-bound arenas. It would take a "constraint
generator" type which would represent multiple arena constraint objects according to given options,
and each of these constraints would be used to create an arena.

Generally, such generalized API needs some way to describe which arena parameters should vary
across the arena set and how, and which should remain uniform. Possible ways for that are
to use some descriptive "language" and/or to generate parameter sets by a function.

In the suggestion, construction arguments of a constraint generator (including value sets
and named patterns) serve as the description language.
Another way could be to add named patterns as possible data values for `task_arena::constraints`;
for example, `tbb::task_arena::constraints c{ .numa_id = tbb::task_arena::iterate }`
would serve as a pattern for generating a set of constraints bound to NUMA domains.

Using a function to generate arena parameter sets seems even more flexible comparing to a description
language, as that function would be not limited in which parameters to alter, and how.

Compared to the proposal, this approach would necessarily be more verbose because of the need to
describe or generate a set of constraints. That could however be mitigated for common use cases
with presets provided by the library allowing for a reasonably concise code. For example:
```c++
  std::vector<tbb::task_arena> arenas = tbb::create_task_arenas(tbb::iterate_over_numa_ids);
```
where `iterate_over_numa_ids` is a predefined variable of a type accepted by the function.

Yet it is questionable if such flexibility is useful at the moment, and so if it justifies
extra complexity. As of now, we know just a few arena set patterns with potential practical usage:
besides NUMA arenas, we can think of splitting CPU resources by core type and of creating a set of
arenas with different priorities. We do not however recall any requests to simplify these use cases.

## Open questions
- Instead of a free-standing function in namespace `tbb`, should we consider
  a static member function in class `task_arena`?
- The proposal does not consider arena priority, simply keeping the default `priority::normal`.
  Are there use cases for pre-setting priorities? Similarly for the experimental thread leave policy.
- Are there more practical use cases which could justify the universal function approach?
- Need to consider alternatives to silently ignoring `numa_id` in constraints, such as an exception
  or undefined behavior.
- Are there any reasons for the API to first go out as an experimental feature?
