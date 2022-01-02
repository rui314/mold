.. _Reader_Writer_Mutexes:

Reader Writer Mutexes
=====================


Mutual exclusion is necessary when at least one thread *writes* to a
shared variable. But it does no harm to permit multiple readers into a
protected region. The reader-writer variants of the mutexes, denoted by
``_rw_`` in the class names, enable multiple readers by distinguishing
*reader locks* from *writer locks.* There can be more than one reader
lock on a given mutex.


Requests for a reader lock are distinguished from requests for a writer
lock via an extra boolean parameter in the constructor for
``scoped_lock``. The parameter is false to request a reader lock and
true to request a writer lock. It defaults to ``true`` so that when
omitted, a ``spin_rw_mutex`` or ``queuing_rw_mutex`` behaves like its
non-``_rw_`` counterpart.

