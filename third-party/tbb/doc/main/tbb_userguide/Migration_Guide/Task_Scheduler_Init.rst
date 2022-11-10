.. _Task_Scheduler_Init:

Migrating from tbb::task_scheduler_init
=======================================

``tbb::task_scheduler_init`` was a multipurpose functionality in the previous versions of Threading
Building Blocks (TBB). This section considers different use cases and how they can be covered with
oneTBB.

Managing the number of threads
---------------------------------------

Querying the default number of threads
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

* `oneapi::tbb::info::default_concurrency()
  <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/info_namespace.html>`_
  returns the maximum concurrency that will be created by *default* in implicit or explicit ``task_arena``.

* `oneapi::tbb::this_task_arena::max_concurrency()
  <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_arena/this_task_arena_ns.html>`_
  returns the maximum number of threads available for the parallel algorithms within the current context
  (or *default* if an implicit arena is not initialized)

* `oneapi::tbb::global_control::active_value(tbb::global_control::max_allowed_parallelism)
  <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/scheduling_controls/global_control_cls.html>`_
  returns the current limit of the thread pool (or *default* if oneTBB scheduler is not initialized)

Setting the maximum concurrency
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

* `task_arena(/* max_concurrency */)
  <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_arena/this_task_arena_ns.html>`_
  limits the maximum concurrency of the parallel algorithm running inside ``task_arena``

* `tbb::global_control(tbb::global_control::max_allowed_parallelism, /* max_concurrency */)
  <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/scheduling_controls/global_control_cls.html>`_
  limits the total number of oneTBB worker threads

Examples
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

The default parallelism:

.. code:: cpp

    #include <oneapi/tbb/info.h>
    #include <oneapi/tbb/parallel_for.h>
    #include <oneapi/tbb/task_arena.h>
    #include <cassert>

    int main() {
        // Get the default number of threads
        int num_threads = oneapi::tbb::info::default_concurrency();

        // Run the default parallelism
        oneapi::tbb::parallel_for( /* ... */ [] {
            // Assert the maximum number of threads
            assert(num_threads == oneapi::tbb::this_task_arena::max_concurrency());
        });

        // Create the default task_arena
        oneapi::tbb::task_arena arena;
        arena.execute([]{
            oneapi::tbb::parallel_for( /* ... */ [] {
                // Assert the maximum number of threads
                assert(num_threads == oneapi::tbb::this_task_arena::max_concurrency());
            });
        });

        return 0;
    }

The limited parallelism:

.. code:: cpp

    #include <oneapi/tbb/info.h>
    #include <oneapi/tbb/parallel_for.h>
    #include <oneapi/tbb/task_arena.h>
    #include <oneapi/tbb/global_control.h>
    #include <cassert>

    int main() {
        // Create the custom task_arena with four threads
        oneapi::tbb::task_arena arena(4);
        arena.execute([]{
            oneapi::tbb::parallel_for( /* ... */ [] {
                // This arena is limited with for threads
                assert(oneapi::tbb::this_task_arena::max_concurrency() == 4);
            });
        });

        // Limit the number of threads to two for all oneTBB parallel interfaces
        oneapi::tbb::global_control global_limit(oneapi::tbb::global_control::max_allowed_parallelism, 2);

        // the default parallelism
        oneapi::tbb::parallel_for( /* ... */ [] {
            // No more than two threads is expected; however, tbb::this_task_arena::max_concurrency() can return a bigger value
            int thread_limit = oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::max_allowed_parallelism);
            assert(thread_limit == 2);
        });

        arena.execute([]{
            oneapi::tbb::parallel_for( /* ... */ [] {
                // No more than two threads is expected; however, tbb::this_task_arena::max_concurrency() is four
                int thread_limit = oneapi::tbb::global_control::active_value(oneapi::tbb::global_control::max_allowed_parallelism);
                assert(thread_limit == 2);
                assert(tbb::this_task_arena::max_concurrency() == 4);
            });
        });

        return 0;
    }

Setting thread stack size
---------------------------------------
Use `oneapi::tbb::global_control(oneapi::tbb::global_control::thread_stack_size, /* stack_size */)
<https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/scheduling_controls/global_control_cls.html>`_
to set the stack size for oneTBB worker threads:

.. code:: cpp

    #include <oneapi/tbb/parallel_for.h>
    #include <oneapi/tbb/global_control.h>

    int main() {
        // Set 16 MB of the stack size for oneTBB worker threads.
        // Note that the stack size of the main thread should be configured in accordace with the
        // system documentation, e.g. at application startup moment
        oneapi::tbb::global_control global_limit(tbb::global_control::thread_stack_size, 16 * 1024 * 1024);

        oneapi::tbb::parallel_for( /* ... */ [] {
            // Create a big array in the stack
            char big_array[10*1024*1024];
        });

        return 0;
    }

Terminating oneTBB scheduler
---------------------------------------
:ref:`task_scheduler_handle_reference`
allows waiting for oneTBB worker threads completion:

.. code:: cpp

    #include <oneapi/tbb/global_control.h>
    #include <oneapi/tbb/parallel_for.h>

    int main() {
        oneapi::tbb::task_scheduler_handle handle{tbb::attach{}};
        // Do some parallel work here
        oneapi::tbb::parallel_for(/* ... */);
        oneapi::tbb::finalize(handle);
        return 0;
    }
