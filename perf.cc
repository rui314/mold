#include "mold.h"

#include <functional>
#include <iomanip>
#include <ios>

#ifndef _WIN32
#include <sys/resource.h>
#include <sys/time.h>
#endif

namespace mold {

i64 Counter::get_value() {
  return values.combine(std::plus());
}

void Counter::print() {
  sort(instances, [](Counter *a, Counter *b) {
    return a->get_value() > b->get_value();
  });

  for (Counter *c : instances)
    std::cout << std::setw(20) << std::right << c->name
              << "=" << c->get_value() << "\n";
}

static i64 now_nsec() {
#ifdef _WIN32
  return (i64)std::chrono::steady_clock::now().time_since_epoch().count();
#else
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (i64)t.tv_sec * 1000000000 + t.tv_nsec;
  #endif
}

#ifdef _WIN32
static i64 to_nsec(FILETIME t) {
  return ((u64)t.dwHighDateTime << 32 + (u64)t.dwLowDateTime) * 100;
}
#else
static i64 to_nsec(struct timeval t) {
  return (i64)t.tv_sec * 1000000000 + t.tv_usec * 1000;
}
#endif

TimerRecord::TimerRecord(std::string name, TimerRecord *parent)
  : name(name), parent(parent) {
#ifdef _WIN32
  FILETIME creation_time, exit_time, kernel_time, user_time;
  GetProcessTimes(GetCurrentProcess(), &creation_time, &exit_time, &kernel_time,
                  &user_time);
#else
  struct rusage usage;
  getrusage(RUSAGE_SELF, &usage);
#endif

  start = now_nsec();
#ifdef _WIN32
  user = to_nsec(user_time);
  sys = to_nsec(kernel_time);
#else
  user = to_nsec(usage.ru_utime);
  sys = to_nsec(usage.ru_stime);
#endif

  if (parent)
    parent->children.push_back(this);
}

void TimerRecord::stop() {
  if (stopped)
    return;
  stopped = true;

#ifdef _WIN32
  FILETIME creation_time, exit_time, kernel_time, user_time;
  GetProcessTimes(GetCurrentProcess(), &creation_time, &exit_time, &kernel_time,
                  &user_time);
#else
  struct rusage usage;
  getrusage(RUSAGE_SELF, &usage);
#endif

  end = now_nsec();
#ifdef _WIN32
  user = to_nsec(user_time) - user;
  sys = to_nsec(kernel_time) - sys;
#else
  user = to_nsec(usage.ru_utime) - user;
  sys = to_nsec(usage.ru_stime) - sys;
#endif
}

static void print_rec(TimerRecord &rec, i64 indent) {
  printf(" % 8.3f % 8.3f % 8.3f  %s%s\n",
         ((double)rec.user / 1000000000),
         ((double)rec.sys / 1000000000),
         (((double)rec.end - rec.start) / 1000000000),
         std::string(indent * 2, ' ').c_str(),
         rec.name.c_str());

  sort(rec.children, [](TimerRecord *a, TimerRecord *b) {
    return a->start < b->start;
  });

  for (TimerRecord *child : rec.children)
    print_rec(*child, indent + 1);
}

void print_timer_records(
    tbb::concurrent_vector<std::unique_ptr<TimerRecord>> &records) {
  for (i64 i = records.size() - 1; i >= 0; i--)
    records[i]->stop();

  for (i64 i = 0; i < records.size(); i++) {
    TimerRecord &inner = *records[i];
    if (inner.parent)
      continue;

    for (i64 j = i - 1; j >= 0; j--) {
      TimerRecord &outer = *records[j];
      if (outer.start <= inner.start && inner.end <= outer.end) {
        inner.parent = &outer;
        outer.children.push_back(&inner);
        break;
      }
    }
  }

  std::cout << "     User   System     Real  Name\n";

  for (std::unique_ptr<TimerRecord> &rec : records)
    if (!rec->parent)
      print_rec(*rec, 0);

  std::cout << std::flush;
}

} // namespace mold
