.. _task_arena_extensions:

task_arena extensions
=====================

.. note::
    To enable these extensions, set the ``TBB_PREVIEW_TASK_GROUP_EXTENSIONS`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

|full_name| implementation extends the `tbb::task_arena specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_arena/task_arena_cls.html>`_ 
with an overload of ``enqueue`` method accepting ``task_handle``. 
   

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
   
           class task_arena {
           public:
               void enqueue(task_handle&& h);        
           }; 

        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. cpp:function:: void enqueue(task_handle&& h)   
     
Enqueues a task owned by ``h`` into the ``task_arena`` for procession.
 
Behavior of this function is identical to generic version (``template<typename F> void task_arena::enqueue(F&& f)``) except parameter type. 

.. note:: 
   ``h`` should not be empty to avoid undefined behavior.
             
.. rubric:: See also

* `oneapi::tbb::task_arena specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_arena/task_arena_cls.html>`_
* :ref:`task_handle` 
