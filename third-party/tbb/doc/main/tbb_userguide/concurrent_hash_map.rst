.. _concurrent_hash_map:

concurrent_hash_map
===================


A ``concurrent_hash_map<Key, T, HashCompare >`` is a hash table that
permits concurrent accesses. The table is a map from a key to a type
``T``. The traits type HashCompare defines how to hash a key and how to
compare two keys.


The following example builds a ``concurrent_hash_map`` where the keys
are strings and the corresponding data is the number of times each
string occurs in the array ``Data``.


::


   #include "oneapi/tbb/concurrent_hash_map.h"
   #include "oneapi/tbb/blocked_range.h"
   #include "oneapi/tbb/parallel_for.h"
   #include <string>
    

   using namespace oneapi::tbb;
   using namespace std;
    

   // Structure that defines hashing and comparison operations for user's type.
   struct MyHashCompare {
       size_t hash( const string& x ) const {
           size_t h = 0;
           for( const char* s = x.c_str(); *s; ++s )
               h = (h*17)^*s;
           return h;
       }
       //! True if strings are equal
       bool equal( const string& x, const string& y ) const {
           return x==y;
       }
   };
    

   // A concurrent hash table that maps strings to ints.
   typedef concurrent_hash_map<string,int,MyHashCompare> StringTable;
    

   // Function object for counting occurrences of strings.
   struct Tally {
       StringTable& table;
       Tally( StringTable& table_ ) : table(table_) {}
       void operator()( const blocked_range<string*> range ) const {
           for( string* p=range.begin(); p!=range.end(); ++p ) {
               StringTable::accessor a;
               table.insert( a, *p );
               a->second += 1;
           }
       }
   };
    

   const size_t N = 1000000;
    

   string Data[N];
    

   void CountOccurrences() {
       // Construct empty table.
       StringTable table;
    

       // Put occurrences into the table
       parallel_for( blocked_range<string*>( Data, Data+N, 1000 ),
                     Tally(table) );
    

       // Display the occurrences
       for( StringTable::iterator i=table.begin(); i!=table.end(); ++i )
           printf("%s %d\n",i->first.c_str(),i->second);
   }


A ``concurrent_hash_map`` acts as a container of elements of type
``std::pair<const Key,T>``. Typically, when accessing a container
element, you are interested in either updating it or reading it. The
template class ``concurrent_hash_map`` supports these two purposes
respectively with the classes ``accessor`` and ``const_accessor`` that
act as smart pointers. An *accessor* represents *update* (*write*)
access. As long as it points to an element, all other attempts to look
up that key in the table block until the ``accessor`` is done. A
``const_accessor`` is similar, except that is represents *read-only*
access. Multiple ``const_accessors`` can point to the same element at
the same time. This feature can greatly improve concurrency in
situations where elements are frequently read and infrequently updated.


The methods ``find`` and ``insert`` take an ``accessor`` or
``const_accessor`` as an argument. The choice tells
``concurrent_hash_map`` whether you are asking for *update* or
*read-only* access. Once the method returns, the access lasts until the
``accessor`` or ``const_accessor`` is destroyed. Because having access
to an element can block other threads, try to shorten the lifetime of
the ``accessor`` or ``const_accessor``. To do so, declare it in the
innermost block possible. To release access even sooner than the end of
the block, use method ``release``. The following example is a rework of
the loop body that uses ``release`` instead of depending upon
destruction to end thread lifetime:


::


           StringTable accessor a;
           for( string* p=range.begin(); p!=range.end(); ++p ) {
               table.insert( a, *p );
               a->second += 1;
               a.release();
           }


The method ``erase(key)`` can also operate concurrently. It implicitly
requests write access. Therefore before erasing the key, it waits on
any other extant accesses on ``key``.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/More_on_HashCompare
