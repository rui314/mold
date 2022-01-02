.. _UpgradeDowngrade:

Upgrade/Downgrade
=================


It is possible to upgrade a reader lock to a writer lock, by using the
method ``upgrade_to_writer``. Here is an example.


::


   std::vector<string> MyVector;
   typedef spin_rw_mutex MyVectorMutexType;
   MyVectorMutexType MyVectorMutex;
    

   void AddKeyIfMissing( const string& key ) {
       // Obtain a reader lock on MyVectorMutex
       MyVectorMutexType::scoped_lock lock(MyVectorMutex,/*is_writer=*/false);
       size_t n = MyVector.size();
       for( size_t i=0; i<n; ++i )
           if( MyVector[i]==key ) return;
       if( !lock.upgrade_to_writer() )
           // Check if key was added while lock was temporarily released
           for( int i=n; i<MyVector.size(); ++i )
              if(MyVector[i]==key ) return; 
       vector.push_back(key);
   }


Note that the vector must sometimes be searched again. This is necessary
because ``upgrade_to_writer`` might have to temporarily release the lock
before it can upgrade. Otherwise, deadlock might ensue, as discussed in
**Lock Pathologies**. Method ``upgrade_to_writer`` returns a ``bool``
that is true if it successfully upgraded the lock without releasing it,
and false if the lock was released temporarily. Thus when
``upgrade_to_writer`` returns false, the code must rerun the search to
check that the key was not inserted by another writer. The example
presumes that keys are always added to the end of the vector, and that
keys are never removed. Because of these assumptions, it does not have
to re-search the entire vector, but only the elements beyond those
originally searched. The key point to remember is that when
``upgrade_to_writer`` returns false, any assumptions established while
holding a reader lock may have been invalidated, and must be rechecked.


For symmetry, there is a corresponding method ``downgrade_to_reader``,
though in practice there are few reasons to use it.

