.. _this_task_arena_extensions:

this_task_arena extensions
==========================

.. note::
    To enable these extensions, set the ``TBB_PREVIEW_TASK_GROUP_EXTENSIONS`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

|full_name| implementation extends the `tbb::this_task_arena specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_arena/this_task_arena_ns.html>`_
with an overloaded ``enqueue`` function. 
   

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/task_arena.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {
            namespace this_task_arena {
                void enqueue(task_handle&& h);
      
                template<typename F>
                void enqueue(F&& f) ;
            } // namespace this_task_arena
        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

   
.. cpp:function:: template<typename F> void enqueue(F&& f)
  
Enqueues a task into the ``task_arena`` currently used by the calling thread to process the specified functor, then immediately returns.
The ``F`` type must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ Standard.

Behavior of this function is identical to ``template<typename F> void task_arena::enqueue(F&& f)`` applied to ``task_arena`` object constructed with ``attach`` parameter.     

.. cpp:function:: void enqueue(task_handle&& h)   
     
Enqueues a task owned by ``h`` into the ``task_arena`` that is currently used by the calling thread.

Behavior of this function is identical to generic version (``template<typename F> void enqueue(F&& f)``) except the parameter type. 

.. note:: 
   ``h`` should not be empty to avoid undefined behavior.
 
        
.. rubric:: See also:

* `oneapi::tbb::this_task_arena specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_arena/this_task_arena_ns.html>`_
* `oneapi::tbb::task_arena specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_arena/task_arena_cls.html>`_
* :ref:`task_handle` 
