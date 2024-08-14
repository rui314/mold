// This file implements HyperLogLog algorithm, which estimates
// the number of unique items in a given multiset.
//
// For more info, read
// https://engineering.fb.com/2018/12/13/data-infrastructure/hyperloglog

#include "common.h"

#include <cmath>

namespace mold {

i64 HyperLogLog::get_cardinality() const {
  double z = 0;
  for (i64 val : buckets)
    z += std::ldexp(1.0, -val);
  return ALPHA * NBUCKETS * NBUCKETS / z;
}

} // namespace mold
