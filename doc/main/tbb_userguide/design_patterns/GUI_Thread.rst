.. _GUI_Thread:

GUI Thread
==========

.. container:: section


   .. rubric:: Problem
      :class: sectiontitle

   A user interface thread must remain responsive to user requests, and
   must not get bogged down in long computations.


.. container:: section


   .. rubric:: Context
      :class: sectiontitle

   Graphical user interfaces often have a dedicated thread ("GUI
   thread") for servicing user interactions. The thread must remain
   responsive to user requests even while the application has long
   computations running. For example, the user might want to press a
   "cancel" button to stop the long running computation. If the GUI
   thread takes part in the long running computation, it will not be
   able to respond to user requests.


.. container:: section


   .. rubric:: Forces
      :class: sectiontitle

   -  The GUI thread services an event loop.


   -  The GUI thread needs to offload work onto other threads without
      waiting for the work to complete.


   -  The GUI thread must be responsive to the event loop and not become
      dedicated to doing the offloaded work.


.. container:: section


   .. rubric:: Related
      :class: sectiontitle

   -  Non-Preemptive Priorities
   -  Local Serializer


.. container:: section


   .. rubric:: Solution
      :class: sectiontitle

   The GUI thread offloads the work by firing off a task to do it using
   method ``task_arena::enqueue`` of a ``task_arena`` instance.
   When finished, the task posts an event to the GUI thread to indicate that the work is done.
   The semantics of ``enqueue`` cause the task to eventually run on a worker thread
   distinct from the calling thread.

   The following figure sketches the communication paths. Items in black are executed 
   by the GUI thread; items in blue are executed by another thread.

   |image0|

.. container:: section


   .. rubric:: Example
      :class: sectiontitle

   The example is for the Microsoft Windows\* operating systems, though
   similar principles apply to any GUI using an event loop idiom. For
   each event, the GUI thread calls a user-defined function ``WndProc`` to process an event.


   ::


      // Event posted from enqueued task when it finishes its work.
      const UINT WM_POP_FOO = WM_USER+0;


      // Queue for transmitting results from enqueued task to GUI thread.
      oneapi::tbb::concurrent_queue<Foo>ResultQueue;


      // GUI thread's private copy of most recently computed result.
      Foo CurrentResult;
       

      LRESULT CALLBACK WndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam) {
         switch(msg) {
             case WM_COMMAND:
                 switch (LOWORD(wParam)) {
                     case IDM_LONGRUNNINGWORK:
                         // User requested a long computation. Delegate it to another thread.
                         LaunchLongRunningWork(hWnd);
                         break;
                     case IDM_EXIT:
                         DestroyWindow(hWnd);
                         break;
                     default:
                         return DefWindowProc(hWnd, msg, wParam, lParam);
                 }
                 break;
             case WM_POP_FOO:
                 // There is another result in ResultQueue for me to grab.
                 ResultQueue.try_pop(CurrentResult);
                 // Update the window with the latest result.
                 RedrawWindow( hWnd, NULL, NULL, RDW_ERASE|RDW_INVALIDATE );
                 break;
             case WM_PAINT: 
                 Repaint the window using CurrentResult
                 break;
             case WM_DESTROY:
                 PostQuitMessage(0);
                 break;
             default:
                 return DefWindowProc( hWnd, msg, wParam, lParam );
         }
         return 0;
      } 


   The GUI thread processes long computations as follows:


   #. The GUI thread calls ``LongRunningWork``, which hands off the work
      to a worker thread and immediately returns.


   #. The GUI thread continues servicing the event loop. If it has to
      repaint the window, it uses the value of\ ``CurrentResult``, which
      is the most recent ``Foo`` that it has seen.


   When a worker finishes the long computation, it pushes the result
   into ResultQueue, and sends a message WM_POP_FOO to the GUI thread.


   #. The GUI thread services a ``WM_POP_FOO`` message by popping an
      item from ResultQueue into CurrentResult. The ``try_pop`` always
      succeeds because there is exactly one ``WM_POP_FOO`` message for
      each item in ``ResultQueue``.


   Routine ``LaunchLongRunningWork`` creates a function task and launches it
   using method ``task_arena::enqueue``.

   ::


      class LongTask {
         HWND hWnd;
         void operator()() {
             Do long computation
             Foo x = result of long computation
             ResultQueue.push( x );
             // Notify GUI thread that result is available.
             PostMessage(hWnd,WM_POP_FOO,0,0);
         }
      public:
         LongTask( HWND hWnd_ ) : hWnd(hWnd_) {}
      };

      void LaunchLongRunningWork( HWND hWnd ) {
         oneapi::tbb::task_arena a;
         a.enqueue(LongTask(hWnd));
      }


   It is essential to use method ``task_arena::enqueue`` here.
   Even though, an explicit ``task_arena`` instance is created,
   the method ``enqueue`` ensures that the function task eventually executes when resources permit,
   even if no thread explicitly waits on the task. In contrast, ``oneapi::tbb::task_group::run`` may
   postpone execution of the function task until it is explicitly waited upon with the ``oneapi::tbb::task_group::wait``.

   The example uses a ``concurrent_queue`` for workers to communicate
   results back to the GUI thread. Since only the most recent result
   matters in the example, and alternative would be to use a shared
   variable protected by a mutex. However, doing so would block the
   worker while the GUI thread was holding a lock on the mutex, and vice
   versa. Using ``concurrent_queue`` provides a simple robust solution.

   If two long computations are in flight, there is a chance that the
   first computation completes after the second one. If displaying the
   result of the most recently requested computation is important, then
   associate a request serial number with the computation. The GUI
   thread can pop from ``ResultQueue`` into a temporary variable, check
   the serial number, and update ``CurrentResult`` only if doing so
   advances the serial number.

.. |image0| image:: Images/image007a.jpg
   :width: 400px
   :height: 150px
