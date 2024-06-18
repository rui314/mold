// Many build systems attempt to invoke as many linker processes as there
// are cores, based on the assumption that the linker is single-threaded.
// However, since mold is multi-threaded, such build systems' behavior is
// not beneficial and just increases the overall peak memory usage.
// On machines with limited memory, this could lead to an out-of-memory
// error.
//
// This file implements a feature that limits the number of concurrent
// mold processes to just 1 for each user. It is intended to be used as
// `MOLD_JOBS=1 ninja` or `MOLD_JOBS=1 make -j$(nproc)`.
//
// We can't use POSIX semaphores because the counter will not be
// decremented automatically when a process exits abnormally. That would
// results in a deadlock. Therefore, we use lockf-based regional file
// locking instead. Unlike POSIX semaphores, the lock will automatically
// released on process termination.
//
// To wake processes that may be waiting on the lock file, we use a
// pthread condition variable. On normal exit, mold sends notifications to
// all waiting processes. In case of abnormal exit, we use
// pthread_cond_timedwait so that waiters will not wait forever.

#include "common.h"

#include <atomic>
#include <fcntl.h>
#include <pthread.h>
#include <pwd.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

namespace mold {

static constexpr i64 MAX_JOBS = 128;

struct SharedData {
  std::atomic_bool initialized;
  pthread_mutex_t mu;
  pthread_cond_t cond;
};

static int num_jobs = -1;
static int lock_fd = -1;
static SharedData *shared_data = nullptr;

static i64 get_mold_jobs() {
  char *env = getenv("MOLD_JOBS");
  if (!env)
    return 0;

  i64 jobs = std::stol(env);
  if (jobs < 0)
    return 0;
  return std::min(jobs, MAX_JOBS);
}

static bool do_lock() {
  for (i64 i = 0; i < num_jobs; i++) {
    lseek(lock_fd, i, SEEK_SET);
    if (lockf(lock_fd, F_TLOCK, 1) == 0)
      return true;
  }
  return false;
}

static SharedData *get_shared_data() {
  // Create a shared memory object and mmap it
  std::string name = "/mold-signal-" + std::to_string(getuid());

  int shm_fd = shm_open(name.c_str(), O_CREAT | O_RDWR, 0600);
  if (shm_fd == -1) {
    perror("shm_open");
    exit(-1);
  }

  i64 size = sizeof(SharedData);
  ftruncate(shm_fd, size);
  SharedData *data = (SharedData *)mmap(0, size, PROT_READ | PROT_WRITE,
                                        MAP_SHARED, shm_fd, 0);
  close(shm_fd);

  if (data->initialized.exchange(true) == false) {
    pthread_mutexattr_t mu_attr;
    pthread_mutexattr_init(&mu_attr);
    pthread_mutexattr_setpshared(&mu_attr, PTHREAD_PROCESS_SHARED);

#ifndef __APPLE__
    pthread_mutexattr_setrobust(&mu_attr, PTHREAD_MUTEX_ROBUST);
#endif

    pthread_mutex_init(&data->mu, &mu_attr);

    pthread_condattr_t cond_attr;
    pthread_condattr_init(&cond_attr);
    pthread_condattr_setpshared(&cond_attr, PTHREAD_PROCESS_SHARED);
    pthread_cond_init(&data->cond, &cond_attr);
  }
  return data;
}

void acquire_global_lock() {
  num_jobs = get_mold_jobs();
  if (num_jobs == 0)
    return;

  shared_data = get_shared_data();

  std::string path;
  if (char *dir = getenv("XDG_RUNTIME_DIR"))
    path = dir + "/mold.lock"s;
  else
    path = "/tmp/mold-" + std::to_string(getuid()) + ".lock";

  lock_fd = open(path.c_str(), O_WRONLY | O_CREAT | O_CLOEXEC, 0600);
  if (lock_fd == -1 || do_lock())
    return;

  pthread_mutex_t *mu = &shared_data->mu;
  pthread_cond_t *cond = &shared_data->cond;
  int r = pthread_mutex_lock(mu);

#ifndef __APPLE__
  // If the previous process got killed while holding the mutex, the
  // mutex has became inconsistent. We need to fix it in that case.
  if (r == EOWNERDEAD)
    pthread_mutex_consistent(mu);
#endif

  for (;;) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += 1;

    int r = pthread_cond_timedwait(cond, mu, &ts);
    if (do_lock() || r != ETIMEDOUT)
      break;
  }

  pthread_mutex_unlock(mu);
}

void release_global_lock() {
  if (lock_fd == -1)
    return;
  close(lock_fd);
  pthread_cond_broadcast(&shared_data->cond);
}

} // namespace mold
