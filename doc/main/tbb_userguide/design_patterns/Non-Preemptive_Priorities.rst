.. _Non-Preemptive_Priorities:

Non-Preemptive Priorities
=========================


.. container:: section


   .. rubric:: Problem
      :class: sectiontitle

   Choose the next work item to do, based on priorities.


.. container:: section


   .. rubric:: Context
      :class: sectiontitle

   The scheduler in |full_name|
   chooses tasks using rules based on scalability concerns. The rules
   are based on the order in which tasks were spawned or enqueued, and
   are oblivious to the contents of tasks. However, sometimes it is best
   to choose work based on some kind of priority relationship.


.. container:: section


   .. rubric:: Forces
      :class: sectiontitle

   -  Given multiple work items, there is a rule for which item should
      be done next that is *not* the default oneTBB rule.


   -  Preemptive priorities are not necessary. If a higher priority item
      appears, it is not necessary to immediately stop lower priority
      items in flight. If preemptive priorities are necessary, then
      non-preemptive tasking is inappropriate. Use threads instead.


.. container:: section


   .. rubric:: Solution
      :class: sectiontitle

   Put the work in a shared work pile. Decouple tasks from specific
   work, so that task execution chooses the actual piece of work to be
   selected from the pile.


.. container:: section


   .. rubric:: Example
      :class: sectiontitle

   The following example implements three priority levels. The user
   interface for it and top-level implementation follow:


   ::


      enum Priority {
         P_High,
         P_Medium,
         P_Low
      };
       

      template<typename Func>
      void EnqueueWork( Priority p, Func f ) {
         WorkItem* item = new ConcreteWorkItem<Func>( p, f );
         ReadyPile.add(item);
      }


   The caller provides a priority ``p`` and a functor ``f`` to routine ``EnqueueWork``.
   The functor may be the result of a lambda expression. ``EnqueueWork`` packages ``f`` as a ``WorkItem`` and adds
   it to global object ``ReadyPile``.


   Class ``WorkItem`` provides a uniform interface for running functors of unknown type:


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
      class ConcreteWorkItem: public WorkItem {
         Func f;
         /*override*/ void run() {
             f();
             delete this;
         }
      public:
         ConcreteWorkItem( Priority p, const Func& f_ ) :
             WorkItem(p), f(f_)
         {}
      };


   Class ``ReadyPile`` contains the core pattern. It maintains a
   collection of work and fires off tasks through the ``oneapi::tbb::task_group::run`` interface
   and then choose a work from the collection:


   ::


      class ReadyPileType {
         // One queue for each priority level
         oneapi::tbb::concurrent_queue<WorkItem*> level[P_Low+1];
         oneapi::tbb::task_group tg;
      public:
         void add( WorkItem* item ) {
             level[item->priority].push(item);
             tg.run(RunWorkItem());
         }
         void runNextWorkItem() {
             // Scan queues in priority order for an item.
             WorkItem* item=NULL;
             for( int i=P_High; i<=P_Low; ++i )
                 if( level[i].try_pop(item) )
                     break;
             assert(item);
             item->run();
         }
      };
       

      ReadyPileType ReadyPile;


   The task added by ``add(item)`` does *not* necessarily execute
   that item. The task itself executes ``runNextWorkItem()``, which may find a
   higher priority item. There is one task for each item, but the
   mapping resolves when the task actually executes, not when it is created.

   Here are the details of class ``RunWorkItem``:

   ::

      class RunWorkItem {
         void operator()() {
             ReadyPile.runNextWorkItem();
         };
      };


   ``RunWorkItem`` objects are fungible. They enable the oneTBB
   scheduler to choose when to do a work item, not which work item to do.


   Other priority schemes can be implemented by changing the internals
   for ``ReadyPileType``. A priority queue could be used to implement
   very fine grained priorities.

   The scalability of the pattern is limited by the scalability of
   ``ReadyPileType``. Ideally scalable concurrent containers should be
   used for it.

