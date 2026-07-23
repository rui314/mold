// This file implements HyperLogLog algorithm, which estimates
// the number of unique items in a given multiset.
//
// For more info, read
// https://engineering.fb.com/2018/12/13/data-infrastructure/hyperloglog

#include "lib.h"

#include <cmath>

namespace mold {

i64 HyperLogLog::get_cardinality() const {
  u8 buckets[NBUCKETS] = {};
  for (const Sketch &sketch : sketches)
    for (i64 i = 0; i < NBUCKETS; i++)
      buckets[i] = std::max(buckets[i], sketch.buckets[i]);

  double z = 0;
  for (i64 val : buckets)
    z += std::ldexp(1.0, -val);
  return ALPHA * NBUCKETS * NBUCKETS / z;
}

} // namespace mold
