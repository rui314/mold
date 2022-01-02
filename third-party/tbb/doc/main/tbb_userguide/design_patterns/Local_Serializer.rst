.. _Local_Serializer:

Local Serializer
================


.. container:: section


   .. rubric:: Context
      :class: sectiontitle

   Consider an interactive program. To maximize concurrency and
   responsiveness, operations requested by the user can be implemented
   as tasks. The order of operations can be important. For example,
   suppose the program presents editable text to the user. There might
   be operations to select text and delete selected text. Reversing the
   order of "select" and "delete" operations on the same buffer would be
   bad. However, commuting operations on different buffers might be
   okay. Hence the goal is to establish serial ordering of tasks
   associated with a given object, but not constrain ordering of tasks
   between different objects.


.. container:: section


   .. rubric:: Forces
      :class: sectiontitle

   -  Operations associated with a certain object must be performed in
      serial order.


   -  Serializing with a lock would be wasteful because threads would be
      waiting at the lock when they could be doing useful work
      elsewhere.


.. container:: section


   .. rubric:: Solution
      :class: sectiontitle

   Sequence the work items using a FIFO (first-in first-out structure).
   Always keep an item in flight if possible. If no item is in flight
   when a work item appears, put the item in flight. Otherwise, push the
   item onto the FIFO. When the current item in flight completes, pop
   another item from the FIFO and put it in flight.


   The logic can be implemented without mutexes, by using
   ``concurrent_queue`` for the FIFO and ``atomic<int>`` to count the
   number of items waiting and in flight. The example explains the
   accounting in detail.


.. container:: section


   .. rubric:: Example
      :class: sectiontitle

   The following example builds on the Non-Preemptive Priorities example
   to implement local serialization in addition to priorities. It
   implements three priority levels and local serializers. The user
   interface for it follows:


   ::


      enum Priority {
         P_High,
         P_Medium,
         P_Low
      };
       

      template<typename Func>
      void EnqueueWork( Priority p, Func f, Serializer* s=NULL );


   Template function ``EnqueueWork`` causes functor ``f`` to run when
   the three constraints in the following table are met.


   .. container:: tablenoborder


      .. list-table:: 
         :header-rows: 1

         * -     Constraint     
           -     Resolved by class...     
         * -     Any prior work for the ``Serializer`` has completed.          
           -     \ ``Serializer``     
         * -     A thread is available.     
           -     \ ``RunWorkItem``     
         * -     No higher priority work is ready to run.     
           -     \ ``ReadyPileType``     




   Constraints on a given functor are resolved from top to bottom in the
   table. The first constraint does not exist when s is NULL. The
   implementation of ``EnqueueWork`` packages the functor in a
   ``SerializedWorkItem`` and routes it to the class that enforces the
   first relevant constraint between pieces of work.


   ::


      template<typename Func>
      void EnqueueWork( Priority p, Func f, Serializer* s=NULL ) {
         WorkItem* item = new SerializedWorkItem<Func>( p, f, s );
         if( s )
             s->add(item);
         else
             ReadyPile.add(item);
      }


   A ``SerializedWorkItem`` is derived from a ``WorkItem``, which serves
   as a way to pass around a prioritized piece of work without knowing
   further details of the work.


   ::


      // Abstract base class for a prioritized piece of work.
      class WorkItem {
      public:
         WorkItem( Priority p ) : priority(p) {}
         // Derived class defines the actual work.
         virtual void run() = 0;
         const Priority priority;
      };
       

      template<typename Func>
      class SerializedWorkItem: public WorkItem {
         Serializer* serializer;
         Func f;
         /*override*/ void run() {
             f();
             Serializer* s = serializer;
             // Destroy f before running Serializer’s next functor.
             delete this;
             if( s )
                 s->noteCompletion();
         }
      public:
         SerializedWorkItem( Priority p, const Func& f_, Serializer* s ) :
             WorkItem(p), serializer(s), f(f_) 
         {}
      };


   Base class ``WorkItem`` is the same as class WorkItem in the example
   for Non-Preemptive Priorities. The notion of serial constraints is
   completely hidden from the base class, thus permitting the framework
   to extend other kinds of constraints or lack of constraints. Class
   ``SerializedWorkItem`` is essentially ``ConcreteWorkItem`` from the
   example for Non-Preemptive Priorities, extended with a ``Serializer``
   aspect.


   Virtual method ``run()`` is invoked when it becomes time to run the
   functor. It performs three steps:


   #. Run the functor.


   #. Destroy the functor.


   #. Notify the ``Serializer`` that the functor completed, and thus
      unconstraining the next waiting functor.


   Step 3 is the difference from the operation of ConcreteWorkItem::run.
   Step 2 could be done after step 3 in some contexts to increase
   concurrency slightly. However, the presented order is recommended
   because if step 2 takes non-trivial time, it likely has side effects
   that should complete before the next functor runs.


   Class ``Serializer`` implements the core of the Local Serializer
   pattern:


   ::


      class Serializer {
         oneapi::tbb::concurrent_queue<WorkItem*> queue;
         std::atomic<int> count;         // Count of queued items and in-flight item
         void moveOneItemToReadyPile() { // Transfer item from queue to ReadyPile
             WorkItem* item;
             queue.try_pop(item);
             ReadyPile.add(item);
         }
      public:
         void add( WorkItem* item ) {
             queue.push(item);
             if( ++count==1 )
                 moveOneItemToReadyPile();
         }
         void noteCompletion() {        // Called when WorkItem completes.
             if( --count!=0 )
                 moveOneItemToReadyPile();
         }
      };


   The class maintains two members:


   -  A queue of WorkItem waiting for prior work to complete.


   -  A count of queued or in-flight work.


   Mutexes are avoided by using ``concurrent_queue<WorkItem*>`` and
   ``atomic<int>`` along with careful ordering of operations. The
   transitions of count are the key understanding how class
   ``Serializer`` works.


   -  If method ``add`` increments ``count`` from 0 to 1, this indicates
      that no other work is in flight and thus the work should be moved
      to the ``ReadyPile``.


   -  If method ``noteCompletion`` decrements count and it is *not* from
      1 to 0, then the queue is non-empty and another item in the queue
      should be moved to ``ReadyPile``.


   Class ``ReadyPile`` is explained in the example for Non-Preemptive
   Priorities.


   If priorities are not necessary, there are two variations on method
   ``moveOneItemToReadyPile``, with different implications.


   -  Method ``moveOneItemToReadyPile`` could directly
      invoke\ ``item->run()``. This approach has relatively low overhead
      and high thread locality for a given ``Serializer``. But it is
      unfair. If the ``Serializer`` has a continual stream of tasks, the
      thread operating on it will keep servicing those tasks to the
      exclusion of others.


   -  Method ``moveOneItemToReadyPile`` could invoke ``task::enqueue``
      to enqueue a task that invokes ``item->run()``. Doing so
      introduces higher overhead and less locality than the first
      approach, but avoids starvation.


   The conflict between fairness and maximum locality is fundamental.
   The best resolution depends upon circumstance.


   The pattern generalizes to constraints on work items more general
   than those maintained by class Serializer. A generalized
   ``Serializer::add`` determines if a work item is unconstrained, and
   if so, runs it immediately. A generalized
   ``Serializer::noteCompletion`` runs all previously constrained items
   that have become unconstrained by the completion of the current work
   item. The term "run" means to run work immediately, or if there are
   more constraints, forwarding the work to the next constraint
   resolver.

