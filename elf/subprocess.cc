#include "mold.h"
#include "../sha.h"

#include <filesystem>
#include <signal.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

#define DAEMON_TIMEOUT 30

namespace mold::elf {

// Exiting from a program with large memory usage is slow --
// it may take a few hundred milliseconds. To hide the latency,
// we fork a child and let it do the actual linking work.
std::function<void()> fork_child() {
  int pipefd[2];
  if (pipe(pipefd) == -1) {
    perror("pipe");
    exit(1);
  }

  pid_t pid = fork();
  if (pid == -1) {
    perror("fork");
    exit(1);
  }

  if (pid > 0) {
    // Parent
    close(pipefd[1]);

    char buf[1];
    if (read(pipefd[0], buf, 1) == 1)
      _exit(0);

    int status;
    waitpid(pid, &status, 0);

    if (WIFEXITED(status))
      _exit(WEXITSTATUS(status));
    if (WIFSIGNALED(status))
      raise(WTERMSIG(status));
    _exit(1);
  }

  // Child
  close(pipefd[0]);

  return [=] {
    char buf[] = {1};
    int n = write(pipefd[1], buf, 1);
    assert(n == 1);
  };
}

static std::string base64(u8 *data, u64 size) {
  static const char chars[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+_";

  std::ostringstream out;

  auto encode = [&](u32 x) {
    out << chars[x & 0b111111]
        << chars[(x >> 6) & 0b111111]
        << chars[(x >> 12) & 0b111111]
        << chars[(x >> 18) & 0b111111];
  };

  i64 i = 0;
  for (; i < size - 3; i += 3)
    encode((data[i + 2] << 16) | (data[i + 1] << 8) | data[i]);

  if (i == size - 1)
    encode(data[i]);
  else if (i == size - 2)
    encode((data[i + 1] << 8) | data[i]);
  return out.str();
}

static std::string compute_sha256(std::span<std::string_view> argv) {
  SHA256_CTX sha;
  SHA256_Init(&sha);

  for (std::string_view arg : argv) {
    if (arg != "-preload" && arg != "--preload") {
      SHA256_Update(&sha, arg.data(), arg.size());
      char buf[] = {0};
      SHA256_Update(&sha, buf, 1);
    }
  }

  u8 digest[SHA256_SIZE];
  SHA256_Final(digest, &sha);
  return base64(digest, SHA256_SIZE);
}

template <typename E>
static void send_fd(Context<E> &ctx, i64 conn, i64 fd) {
  struct iovec iov;
  char dummy = '1';
  iov.iov_base = &dummy;
  iov.iov_len = 1;

  struct msghdr msg = {};
  msg.msg_iov = &iov;
  msg.msg_iovlen = 1;

  char buf[CMSG_SPACE(sizeof(int))];
  msg.msg_control = buf;
  msg.msg_controllen = CMSG_LEN(sizeof(int));

  struct cmsghdr *cmsg = CMSG_FIRSTHDR(&msg);
  cmsg->cmsg_level = SOL_SOCKET;
  cmsg->cmsg_type = SCM_RIGHTS;
  cmsg->cmsg_len = CMSG_LEN(sizeof(int));
  *(int *)CMSG_DATA(cmsg) = fd;

  if (sendmsg(conn, &msg, 0) == -1)
    Fatal(ctx) << "sendmsg failed: " << errno_string();
}

template <typename E>
static i64 recv_fd(Context<E> &ctx, i64 conn) {
  struct iovec iov;
  char buf[1];
  iov.iov_base = buf;
  iov.iov_len = sizeof(buf);

  struct msghdr msg = {};
  msg.msg_iov = &iov;
  msg.msg_iovlen = 1;

  char cmsgbuf[CMSG_SPACE(sizeof(int))];
  msg.msg_control = (caddr_t)cmsgbuf;
  msg.msg_controllen = sizeof(cmsgbuf);

  i64 len = recvmsg(conn, &msg, 0);
  if (len <= 0)
    Fatal(ctx) << "recvmsg failed: " << errno_string();

  struct cmsghdr *cmsg;
  cmsg = CMSG_FIRSTHDR(&msg);
  return *(int *)CMSG_DATA(cmsg);
}

template <typename E>
void try_resume_daemon(Context<E> &ctx) {
  i64 conn = socket(AF_UNIX, SOCK_STREAM, 0);
  if (conn == -1)
    Fatal(ctx) << "socket failed: " << errno_string();

  std::string path = "/tmp/mold-" + compute_sha256(ctx.cmdline_args);

  struct sockaddr_un name = {};
  name.sun_family = AF_UNIX;
  memcpy(name.sun_path, path.data(), path.size());

  if (connect(conn, (struct sockaddr *)&name, sizeof(name)) != 0) {
    close(conn);
    return;
  }

  send_fd(ctx, conn, STDOUT_FILENO);
  send_fd(ctx, conn, STDERR_FILENO);

  char buf[1];
  i64 r = read(conn, buf, 1);
  close(conn);
  if (r == 1)
    exit(0);
}

template <typename E>
void daemonize(Context<E> &ctx, std::function<void()> *wait_for_client,
               std::function<void()> *on_complete) {
  if (daemon(1, 0) == -1)
    Fatal(ctx) << "daemon failed: " << errno_string();

  i64 sock = socket(AF_UNIX, SOCK_STREAM, 0);
  if (sock == -1)
    Fatal(ctx) << "socket failed: " << errno_string();

  socket_tmpfile =
    strdup(("/tmp/mold-" + compute_sha256(ctx.cmdline_args)).c_str());

  struct sockaddr_un name = {};
  name.sun_family = AF_UNIX;
  strcpy(name.sun_path, socket_tmpfile);

  u32 orig_mask = umask(0177);

  if (bind(sock, (struct sockaddr *)&name, sizeof(name)) == -1) {
    if (errno != EADDRINUSE)
      Fatal(ctx) << "bind failed: " << errno_string();

    unlink(socket_tmpfile);
    if (bind(sock, (struct sockaddr *)&name, sizeof(name)) == -1)
      Fatal(ctx) << "bind failed: " << errno_string();
  }

  umask(orig_mask);

  if (listen(sock, 0) == -1)
    Fatal(ctx) << "listen failed: " << errno_string();

  static i64 conn = -1;

  *wait_for_client = [=, &ctx] {
    fd_set rfds;
    FD_ZERO(&rfds);
    FD_SET(sock, &rfds);

    struct timeval tv;
    tv.tv_sec = DAEMON_TIMEOUT;
    tv.tv_usec = 0;

    i64 res = select(sock + 1, &rfds, NULL, NULL, &tv);
    if (res == -1)
      Fatal(ctx) << "select failed: " << errno_string();

    if (res == 0) {
      std::cout << "timeout\n";
      exit(0);
    }

    conn = accept(sock, NULL, NULL);
    if (conn == -1)
      Fatal(ctx) << "accept failed: " << errno_string();
    unlink(socket_tmpfile);

    dup2(recv_fd(ctx, conn), STDOUT_FILENO);
    dup2(recv_fd(ctx, conn), STDERR_FILENO);
  };

  *on_complete = [=] {
    char buf[] = {1};
    int n = write(conn, buf, 1);
    assert(n == 1);
  };
}

static std::string get_self_path() {
  return std::filesystem::read_symlink("/proc/self/exe");
}

template <typename E>
std::string find_dso(Context<E> &ctx, std::filesystem::path self) {
  // Look for mold-wrapper.so from the same directory as the executable is.
  std::filesystem::path path = self.parent_path() / "mold-wrapper.so";
  std::error_code ec;
  if (std::filesystem::is_regular_file(path, ec) && !ec)
    return path;

#ifdef LIBDIR
  // If not found, search $(LIBDIR)/mold, which is /usr/local/lib/mold
  // by default.
  path = LIBDIR "/mold/mold-wrapper.so";
  if (std::filesystem::is_regular_file(path, ec) && !ec)
    return path;
#endif

  // Look for ../lib/mold/mold-wrapper.so
  path = self.parent_path() / "../lib/mold/mold-wrapper.so";
  if (std::filesystem::is_regular_file(path, ec) && !ec)
    return path;

  Fatal(ctx) << "mold-wrapper.so is missing";
}

template <typename E>
[[noreturn]]
void process_run_subcommand(Context<E> &ctx, int argc, char **argv) {
  std::string_view arg1 = argv[1];
  assert(arg1 == "-run" || arg1 == "--run");
  if (!argv[2])
    Fatal(ctx) << "-run: argument missing";

  // Get the mold-wrapper.so path
  std::string self = get_self_path();
  std::string dso_path = find_dso(ctx, self);

  // Set environment variables
  putenv(strdup(("LD_PRELOAD=" + dso_path).c_str()));
  putenv(strdup(("MOLD_PATH=" + self).c_str()));

  // If ld, ld.lld or ld.gold is specified, run mold itself
  if (std::string cmd = filepath(argv[2]).filename();
      cmd == "ld" || cmd == "ld.lld" || cmd == "ld.gold") {
    std::vector<char *> args;
    args.push_back(argv[0]);
    args.insert(args.end(), argv + 3, argv + argc);
    args.push_back(nullptr);
    execv(self.c_str(), args.data());
    Fatal(ctx) << "mold -run failed: " << self << ": " << errno_string();
  }

  // Execute a given command
  execvp(argv[2], argv + 2);
  Fatal(ctx) << "mold -run failed: " << argv[2] << ": " << errno_string();
}

#define INSTANTIATE(E)                                                  \
  template void try_resume_daemon(Context<E> &);                        \
  template void daemonize(Context<E> &, std::function<void()> *,        \
                          std::function<void()> *);                     \
  template void process_run_subcommand(Context<E> &, int, char **)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(ARM64);
INSTANTIATE(ARM32);
INSTANTIATE(RISCV64);

} // namespace mold::elf
