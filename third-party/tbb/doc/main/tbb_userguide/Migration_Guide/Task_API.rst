.. _Task_API:

Migrating from low-level task API
=================================

The low-level task API of Intel(R) Threading Building Blocks (TBB) was considered complex and hence
error-prone, which was the primary reason it had been removed from oneAPI Threading Building Blocks
(oneTBB). This guide helps with the migration from TBB to oneTBB for the use cases where low-level
task API is used.

Spawning of individual tasks
----------------------------
For most use cases, the spawning of individual tasks can be replaced with the use of either
``oneapi::tbb::task_group`` or ``oneapi::tbb::parallel_invoke``.

For example, ``RootTask``, ``ChildTask1``, and ``ChildTask2`` are the user-side functors that
inherit ``tbb::task`` and implement its interface. Then spawning of ``ChildTask1`` and
``ChildTask2`` tasks that can execute in parallel with each other and waiting on the ``RootTask`` is
implemented as:

.. code:: cpp

    #include <tbb/task.h>

    int main() {
        // Assuming RootTask, ChildTask1, and ChildTask2 are defined.
        RootTask& root = *new(tbb::task::allocate_root()) RootTask{};

        ChildTask1& child1 = *new(root.allocate_child()) ChildTask1{/*params*/};
        ChildTask2& child2 = *new(root.allocate_child()) ChildTask2{/*params*/};

        root.set_ref_count(3);
        
        tbb::task::spawn(child1);
        tbb::task::spawn(child2);

        root.wait_for_all();
    }


Using ``oneapi::tbb::task_group``
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
The code above can be rewritten using ``oneapi::tbb::task_group``:

.. code:: cpp

    #include <oneapi/tbb/task_group.h>

    int main() {
        // Assuming ChildTask1, and ChildTask2 are defined.
        oneapi::tbb::task_group tg;
        tg.run(ChildTask1{/*params*/});
        tg.run(ChildTask2{/*params*/});
        tg.wait();
    }

The code looks more concise now. It also enables lambda functions and does not require you to
implement ``tbb::task`` interface that overrides the ``tbb::task* tbb::task::execute()`` virtual
method. With this new approach, you work with functors in a C++-standard way by implementing ``void
operator() const``:

.. code:: cpp

    struct Functor {
        // Member to be called when object of this type are passed into
        // oneapi::tbb::task_group::run() method
        void operator()() const {}
    };


Using ``oneapi::tbb::parallel_invoke``
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
It is also possible to use ``oneapi::tbb::parallel_invoke`` to rewrite the original code and make it
even more concise:

.. code:: cpp

    #include <oneapi/tbb/parallel_invoke.h>

    int main() {
        // Assuming ChildTask1, and ChildTask2 are defined.
        oneapi::tbb::parallel_invoke(
            ChildTask1{/*params*/},
            ChildTask2{/*params*/}
        );
    }


Adding more work during task execution
--------------------------------------
``oneapi::tbb::parallel_invoke`` follows a blocking style of programming, which means that it
completes only when all functors passed to the parallel pattern complete their execution.

In TBB, cases when the amount of work is not known in advance and the work needs to be added during
the execution of a parallel algorithm were mostly covered by ``tbb::parallel_do`` high-level
parallel pattern. The ``tbb::parallel_do`` algorithm logic may be implemented using the task API as:

.. code:: cpp

    #include <cstddef>
    #include <vector>
    #include <tbb/task.h>

    // Assuming RootTask and OtherWork are defined and implement tbb::task interface.

    struct Task : public tbb::task {
        Task(tbb::task& root, int i)
            : m_root(root), m_i(i)
        {}

        tbb::task* execute() override {
            // ... do some work for item m_i ...

            if (add_more_parallel_work) {
                tbb::task& child = *new(m_root.allocate_child()) OtherWork;
                tbb::task::spawn(child);
            }
            return nullptr;
        }

        tbb::task& m_root;
        int m_i;
    };

    int main() {
        std::vector<int> items = { 0, 1, 2, 3, 4, 5, 6, 7 };
        RootTask& root = *new(tbb::task::allocate_root()) RootTask{/*params*/};
        
        root.set_ref_count(items.size() + 1);
        
        for (std::size_t i = 0; i < items.size(); ++i) {
            Task& task = *new(root.allocate_child()) Task(root, items[i]);
            tbb::task::spawn(task);
        }

        root.wait_for_all();
        return 0;
    }

In oneTBB ``tbb::parallel_do`` interface was removed. Instead, the functionality of adding new work
was included into the ``oneapi::tbb::parallel_for_each`` interface.

The previous use case can be rewritten in oneTBB as follows:

.. code:: cpp

    #include <vector>
    #include <oneapi/tbb/parallel_for_each.h>

    int main() {
        std::vector<int> items = { 0, 1, 2, 3, 4, 5, 6, 7 };

        oneapi::tbb::parallel_for_each(
            items.begin(), items.end(),
            [](int& i, tbb::feeder<int>& feeder) {

                // ... do some work for item i ...

                if (add_more_parallel_work)
                    feeder.add(i);
            }
        );
    }

Since both TBB and oneTBB support nested expressions, you can run additional functors from within an
already running functor.

The previous use case can be rewritten using ``oneapi::tbb::task_group`` as:

.. code:: cpp

    #include <cstddef>
    #include <vector>
    #include <oneapi/tbb/task_group.h>

    int main() {
        std::vector<int> items = { 0, 1, 2, 3, 4, 5, 6, 7 };

        oneapi::tbb::task_group tg;
        for (std::size_t i = 0; i < items.size(); ++i) {
            tg.run([&i = items[i], &tg] {

                // ... do some work for item i ...

                if (add_more_parallel_work)
                    // Assuming OtherWork is defined.
                    tg.run(OtherWork{});

            });
        }
        tg.wait();
    }


Task recycling
--------------
You can re-run the functor by passing ``*this`` to the ``oneapi::tbb::task_group::run()``
method. The functor will be copied in this case. However, its state can be shared among instances:

.. code:: cpp

    #include <memory>
    #include <oneapi/tbb/task_group.h>

    struct SharedStateFunctor {
        std::shared_ptr<Data> m_shared_data;
        oneapi::tbb::task_group& m_task_group;

        void operator()() const {
            // do some work processing m_shared_data

            if (has_more_work)
                m_task_group.run(*this);

            // Note that this might be concurrently accessing m_shared_data already
        }
    };

    int main() {
        // Assuming Data is defined.
        std::shared_ptr<Data> data = std::make_shared<Data>(/*params*/);
        oneapi::tbb::task_group tg;
        tg.run(SharedStateFunctor{data, tg});
        tg.wait();
    }

Such patterns are particularly useful when the work within a functor is not completed but there is a
need for the task scheduler to react to outer circumstances, such as cancellation of group
execution. To avoid issues with concurrent access, it is recommended to submit it for re-execution
as the last step:

.. code:: cpp

    #include <memory>
    #include <oneapi/tbb/task_group.h>

    struct SharedStateFunctor {
        std::shared_ptr<Data> m_shared_data;
        oneapi::tbb::task_group& m_task_group;

        void operator()() const {
            // do some work processing m_shared_data

            if (need_to_yield) {
                m_task_group.run(*this);
                return;
            }
        }
    };

    int main() {
        // Assuming Data is defined.
        std::shared_ptr<Data> data = std::make_shared<Data>(/*params*/);
        oneapi::tbb::task_group tg;
        tg.run(SharedStateFunctor{data, tg});
        tg.wait();
    }

   
Recycling as child or continuation
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
In oneTBB this kind of recycling is done manually. You have to track when it is time to run the
task:

.. code:: cpp
          
    #include <cstddef>
    #include <vector>
    #include <atomic>
    #include <cassert>
    #include <oneapi/tbb/task_group.h>

    struct ContinuationTask {
        ContinuationTask(std::vector<int>& data, int& result)
            : m_data(data), m_result(result)
        {}

        void operator()() const {
            for (const auto& item : m_data)
                m_result += item;
        }

        std::vector<int>& m_data;
        int& m_result;
    };

    struct ChildTask {
        ChildTask(std::vector<int>& data, int& result,
                  std::atomic<std::size_t>& tasks_left, std::atomic<std::size_t>& tasks_done,
                  oneapi::tbb::task_group& tg)
            : m_data(data), m_result(result), m_tasks_left(tasks_left), m_tasks_done(tasks_done), m_tg(tg)
        {}

        void operator()() const {
            std::size_t index = --m_tasks_left;
            m_data[index] = produce_item_for(index);
            std::size_t done_num = ++m_tasks_done;
            if (index % 2 != 0) {
                // Recycling as child
                m_tg.run(*this);
                return;
            } else if (done_num == m_data.size()) {
                assert(m_tasks_left == 0);
                // Spawning a continuation that does reduction
                m_tg.run(ContinuationTask(m_data, m_result));
            }
        }
        std::vector<int>& m_data;
        int& m_result;
        std::atomic<std::size_t>& m_tasks_left;
        std::atomic<std::size_t>& m_tasks_done;
        oneapi::tbb::task_group& m_tg;
    };


    int main() {
        int result = 0;
        std::vector<int> items(10, 0);
        std::atomic<std::size_t> tasks_left{items.size()};
        std::atomic<std::size_t> tasks_done{0};

        oneapi::tbb::task_group tg;
        for (std::size_t i = 0; i < items.size(); i+=2) {
            tg.run(ChildTask(items, result, tasks_left, tasks_done, tg));
        }
        tg.wait();
    }


Scheduler Bypass
----------------

TBB ``task::execute()`` method can return a pointer to a task that can be executed next by the current thread.
This might reduce scheduling overheads compared to direct ``spawn``. Similar to ``spawn``, the returned task 
is not guaranteed to be executed next by the current thread.

.. code:: cpp
    
    #include <tbb/task.h>
    
    // Assuming OtherTask is defined.
    
    struct Task : tbb::task {
        task* execute(){
            // some work to do ...
            
            auto* other_p = new(this->parent().allocate_child()) OtherTask{};
            this->parent().add_ref_count();
            
            return other_p;
        }
    };
    
    int main(){
        // Assuming RootTask is  defined.
        RootTask& root = *new(tbb::task::allocate_root()) RootTask{};
    
        Task& child = *new(root.allocate_child()) Task{/*params*/};
        
        root.add_ref_count();
        
        tbb::task_spawn(child);
        
        root.wait_for_all();
    }

In oneTBB, this can be done using ``oneapi::tbb::task_group``. 

.. code:: cpp
   
    #include <oneapi/tbb/task_group.h>
    
    // Assuming OtherTask is defined.
    
    int main(){
        oneapi::tbb::task_group tg;
        
        tg.run([&tg](){
            //some work to do ...
            
            return tg.defer(OtherTask{});
        });
        
        tg.wait();
    }

Here ``oneapi::tbb::task_group::defer`` adds a new task into the ``tg``. However, the task is not put into a 
queue of tasks ready for execution via ``oneapi::tbb::task_group::run``, but bypassed to the executing thread directly 
via function return value. 

Deferred task creation
----------------------
The TBB low-level task API separates the task creation from the actual spawning. This separation allows to
postpone the task spawning, while the parent task and final result production are blocked from premature leave. 
For example, ``RootTask``, ``ChildTask``, and ``CallBackTask`` are the user-side functors that
inherit ``tbb::task`` and implement its interface. Then, blocking the ``RootTask`` from leaving prematurely
and waiting on it is implemented as follows: 

.. code:: cpp

    #include <tbb/task.h>

    int main() {
        // Assuming RootTask, ChildTask, and CallBackTask are defined.
        RootTask& root = *new(tbb::task::allocate_root()) RootTask{};

        ChildTask&    child    = *new(root.allocate_child()) ChildTask{/*params*/};
        CallBackTask& cb_task  = *new(root.allocate_child()) CallBackTask{/*params*/};

        root.set_ref_count(3);
        
        tbb::task::spawn(child);
        
        register_callback([cb_task&](){
            tbb::task::enqueue(cb_task);
        });

        root.wait_for_all();
        // Control flow will reach here only after both ChildTask and CallBackTask are executed,
        // i.e. after the callback is called  
    }

In oneTBB, this can be done using ``oneapi::tbb::task_group``.

.. code:: cpp

    #include <oneapi/tbb/task_group.h>

    int main(){
        oneapi::tbb::task_group tg;
        oneapi::tbb::task_arena arena;
        // Assuming ChildTask and CallBackTask are defined.
        
        auto cb = tg.defer(CallBackTask{/*params*/});
        
        register_callback([&tg, c = std::move(cb), &arena]{
            arena.enqueue(c);
        });        

        tg.run(ChildTask{/*params*/});

 
        tg.wait();
        // Control flow gets here once both ChildTask and CallBackTask are executed
        // i.e. after the callback is called  
    }

Here ``oneapi::tbb::task_group::defer`` adds a new task into the ``tg``. However, the task is not spawned until 
``oneapi::tbb::task_arena::enqueue`` is called. 

.. note::
   The call to ``oneapi::tbb::task_group::wait`` will not return control until both ``ChildTask`` and 
   ``CallBackTask`` are executed.
