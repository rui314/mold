#define _GNU_SOURCE 1

#include <dlfcn.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#if __has_include(<alloca.h>)
# include <alloca.h>
#endif

#ifdef __linux__
# include <sys/syscall.h>

// glibc defines these only since 2.27.
# ifndef MFD_CLOEXEC
#  define MFD_CLOEXEC 1
#  define MFD_ALLOW_SEALING 2
# endif
# ifndef F_ADD_SEALS
#  define F_ADD_SEALS 1033
#  define F_GET_SEALS 1034
#  define F_SEAL_SEAL 1
#  define F_SEAL_SHRINK 2
#  define F_SEAL_GROW 4
#  define F_SEAL_WRITE 8
# endif
#endif

extern char **environ;

static char *get_mold_path() {
  char *path = getenv("MOLD_PATH");
  if (path)
    return path;
  fprintf(stderr, "MOLD_PATH is not set\n");
  exit(1);
}

static void debug_print(const char *fmt, ...) {
  if (!getenv("MOLD_WRAPPER_DEBUG"))
    return;

  va_list ap;
  va_start(ap, fmt);
  fprintf(stderr, "mold-wrapper.so: ");
  vfprintf(stderr, fmt, ap);
  fflush(stderr);
  va_end(ap);
}

static int count_args(va_list *ap) {
  va_list aq;
  va_copy(aq, *ap);

  int i = 0;
  while (va_arg(aq, char *))
    i++;
  va_end(aq);
  return i;
}

static void copy_args(char **argv, const char *arg0, va_list *ap) {
  int i = 1;
  char *arg;
  while ((arg = va_arg(*ap, char *)))
    argv[i++] = arg;

  ((const char **)argv)[0] = arg0;
  ((const char **)argv)[i] = NULL;
}

static bool is_ld(const char *path) {
  const char *ptr = path + strlen(path);
  while (path < ptr && ptr[-1] != '/')
    ptr--;

  return !strcmp(ptr, "ld") || !strcmp(ptr, "ld.lld") ||
         !strcmp(ptr, "ld.gold") || !strcmp(ptr, "ld.bfd") ||
         !strcmp(ptr, "ld.mold");
}

// `mold -run` passes this library to the command in a sealed memfd whose
// descriptor number is in MOLD_WRAPPER_FD. Every process that the command
// starts inherits the descriptor and preloads the library from it, with
// LD_PRELOAD=/proc/self/fd/<fd> on Linux or LD_PRELOAD_FDS=<fd> on FreeBSD.
//
// A process may close the descriptor or reuse its number for another file
// and then run a program; Python's subprocess module, for example, closes
// all descriptors in a child. The program would then fail to start on
// FreeBSD, or preload whatever file has that number. So, before running a
// program, we check the descriptor. If it has been closed, we put a new
// copy of the library there, which we create from the image that we map at
// startup. If another file has taken its number, which is rare, we remove
// the library from the program's environment.
#ifdef __FreeBSD__
# define PRELOAD_VAR "LD_PRELOAD_FDS"
# define PRELOAD_PREFIX ""
#else
# define PRELOAD_VAR "LD_PRELOAD"
# define PRELOAD_PREFIX "/proc/self/fd/"
#endif

static int wrapper_fd = -1;
static const char *image;
static size_t image_size;

// The item in PRELOAD_VAR that refers to `wrapper_fd`.
static char preload_item[32];

__attribute__((constructor))
static void init() {
  char *val = getenv("MOLD_WRAPPER_FD");
  if (!val)
    return;

  int fd = atoi(val);
  struct stat st;
  if (fstat(fd, &st) == -1)
    return;

  void *p = mmap(NULL, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
  if (p == MAP_FAILED)
    return;

  wrapper_fd = fd;
  image = p;
  image_size = st.st_size;
  snprintf(preload_item, sizeof(preload_item), PRELOAD_PREFIX "%d", fd);
}

static int create_memfd() {
#ifdef __linux__
  // glibc provides memfd_create() only since 2.27.
  return syscall(SYS_memfd_create, "mold-wrapper.so",
                 MFD_CLOEXEC | MFD_ALLOW_SEALING);
#else
  return memfd_create("mold-wrapper.so", MFD_CLOEXEC | MFD_ALLOW_SEALING);
#endif
}

// Returns true if `wrapper_fd` is a sealed file with our contents.
static bool is_wrapper_fd() {
  int seals = fcntl(wrapper_fd, F_GET_SEALS);
  int want = F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE;
  if (seals == -1 || (seals & want) != want)
    return false;

  struct stat st;
  if (fstat(wrapper_fd, &st) == -1 || (size_t)st.st_size != image_size)
    return false;

  void *p = mmap(NULL, image_size, PROT_READ, MAP_PRIVATE, wrapper_fd, 0);
  if (p == MAP_FAILED)
    return false;
  bool eq = !memcmp(p, image, image_size);
  munmap(p, image_size);
  return eq;
}

// Puts a copy of the library at `wrapper_fd` if it is free. Returns true
// on success.
static bool restore_wrapper_fd() {
  int fd = create_memfd();
  if (fd == -1)
    return false;

  // F_DUPFD returns the lowest free descriptor not less than `wrapper_fd`.
  // Unlike the one from memfd_create(), programs inherit it.
  int seals = F_SEAL_SEAL | F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE;
  int copy = -1;
  if (write(fd, image, image_size) == (ssize_t)image_size &&
      fcntl(fd, F_ADD_SEALS, seals) == 0)
    copy = fcntl(fd, F_DUPFD, wrapper_fd);
  close(fd);

  if (copy == wrapper_fd) {
    debug_print("restored descriptor %d\n", wrapper_fd);
    return true;
  }
  if (copy != -1)
    close(copy);
  debug_print("cannot restore descriptor %d\n", wrapper_fd);
  return false;
}

static int count_env(char *const *envp) {
  int i = 0;
  while (envp[i])
    i++;
  return i;
}

// Returns true if `entry` is an environment entry for `name`.
static bool is_var(const char *entry, const char *name) {
  size_t len = strlen(name);
  return !strncmp(entry, name, len) && entry[len] == '=';
}

// Returns the total size of the PRELOAD_VAR entries in `envp`.
static size_t preload_size(char *const *envp) {
  size_t size = 0;
  for (; *envp; envp++)
    if (is_var(*envp, PRELOAD_VAR))
      size += strlen(*envp) + 1;
  return size;
}

// Copies `envp` to `env` without the references to `wrapper_fd`, writing
// new PRELOAD_VAR entries to `buf`.
static void remove_wrapper(char **env, char *buf, char *const *envp) {
  size_t itemlen = strlen(preload_item);
  size_t namelen = strlen(PRELOAD_VAR "=");

  for (; *envp; envp++) {
    if (is_var(*envp, "MOLD_WRAPPER_FD"))
      continue;

    if (is_var(*envp, PRELOAD_VAR)) {
      // Copy the items except ours, each with the space or colon after it.
      *env++ = buf;
      memcpy(buf, *envp, namelen);
      buf += namelen;

      for (char *s = *envp + namelen; *s;) {
        size_t len = strcspn(s, " :");
        size_t n = s[len] ? len + 1 : len;
        if (len != itemlen || memcmp(s, preload_item, len)) {
          memcpy(buf, s, n);
          buf += n;
        }
        s += n;
      }
      *buf++ = '\0';
    } else {
      *env++ = *envp;
    }
  }
  *env = NULL;
}

// Removes this library from `envp` if a program cannot preload it from
// `wrapper_fd`. The new environment is on the caller's stack because
// malloc() is not safe in a child that vfork() created.
#define PREPARE_ENV(envp)                                                 \
  if (image && !is_wrapper_fd() && !restore_wrapper_fd()) {               \
    char **env_ = alloca((count_env(envp) + 1) * sizeof(char *));         \
    char *buf_ = alloca(preload_size(envp));                              \
    remove_wrapper(env_, buf_, envp);                                     \
    envp = env_;                                                          \
  }

int execvpe(const char *file, char *const *argv, char *const *envp) {
  debug_print("execvpe %s\n", file);

  if (!strcmp(file, "ld") || is_ld(file))
    file = get_mold_path();

  for (int i = 0; envp[i]; i++)
    putenv(envp[i]);

  // execvp() takes the environment from `environ`.
  char **orig = environ;
  char *const *env = environ;
  PREPARE_ENV(env);
  environ = (char **)env;

  typeof(execvpe) *real = dlsym(RTLD_NEXT, "execvp");
  int ret = real(file, argv, environ);
  environ = orig;
  return ret;
}

int execve(const char *path, char *const *argv, char *const *envp) {
  debug_print("execve %s\n", path);
  if (is_ld(path))
    path = get_mold_path();
  PREPARE_ENV(envp);
  typeof(execve) *real = dlsym(RTLD_NEXT, "execve");
  return real(path, argv, envp);
}

int execl(const char *path, const char *arg0, ...) {
  va_list ap;
  va_start(ap, arg0);
  char **argv = alloca((count_args(&ap) + 2) * sizeof(char *));
  copy_args(argv, arg0, &ap);
  va_end(ap);
  return execve(path, argv, environ);
}

int execlp(const char *file, const char *arg0, ...) {
  va_list ap;
  va_start(ap, arg0);
  char **argv = alloca((count_args(&ap) + 2) * sizeof(char *));
  copy_args(argv, arg0, &ap);
  va_end(ap);
  return execvpe(file, argv, environ);
}

int execle(const char *path, const char *arg0, ...) {
  va_list ap;
  va_start(ap, arg0);
  char **argv = alloca((count_args(&ap) + 2) * sizeof(char *));
  copy_args(argv, arg0, &ap);
  char **env = va_arg(ap, char **);
  va_end(ap);
  return execve(path, argv, env);
}

int execv(const char *path, char *const *argv) {
  return execve(path, argv, environ);
}

int execvp(const char *file, char *const *argv) {
  return execvpe(file, argv, environ);
}

int posix_spawn(pid_t *pid, const char *path,
                const posix_spawn_file_actions_t *file_actions,
                const posix_spawnattr_t *attrp,
                char *const *argv, char *const *envp) {
  debug_print("posix_spawn %s\n", path);
  if (is_ld(path))
    path = get_mold_path();
  PREPARE_ENV(envp);
  typeof(posix_spawn) *real = dlsym(RTLD_NEXT, "posix_spawn");
  return real(pid, path, file_actions, attrp, argv, envp);
}

int posix_spawnp(pid_t *pid, const char *file,
		 const posix_spawn_file_actions_t *file_actions,
		 const posix_spawnattr_t *attrp,
		 char *const *argv, char *const *envp) {
  debug_print("posix_spawnp %s\n", file);
  if (is_ld(file))
    file = get_mold_path();
  PREPARE_ENV(envp);
  typeof(posix_spawnp) *real = dlsym(RTLD_NEXT, "posix_spawnp");
  return real(pid, file, file_actions, attrp, argv, envp);
}
