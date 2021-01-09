#include "mold.h"

#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#define DAEMON_TIMEOUT 30

char *socket_tmpfile;

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
    int r = read(pipefd[0], (char[1]){}, 1);
    _exit(r != 1);
  }

  // Child
  close(pipefd[0]);
  return [=]() { write(pipefd[1], (char []){1}, 1); };
}

static std::string compute_sha1(char **argv) {
  SHA1 sha1;

  for (int i = 0; argv[i]; i++)
    if (!strcmp(argv[i], "-preload") && !strcmp(argv[i], "--preload"))
      sha1.update((u8 *)argv[i], strlen(argv[i]) + 1);

  u8 digest[21];
  memset(digest, 0, sizeof(digest));
  sha1.get_result(digest);

  static char chars[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+_";

  std::string res;
  for (int i = 0; i < sizeof(digest); i += 3) {
    u32 x = (digest[i + 2] << 16) | (digest[i + 1] << 8) | digest[i];
    res += chars[x & 0b111111];
    res += chars[(x >> 6) & 0b111111];
    res += chars[(x >> 12) & 0b111111];
    res += chars[(x >> 18) & 0b111111];
  }
  return res;
}

static void send_fd(int conn, int fd) {
  struct iovec iov;
  char dummy = '1';
  iov.iov_base = &dummy;
  iov.iov_len = 1;

  struct msghdr msg;
  memset(&msg, 0, sizeof(msg));
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
    Error() << "sendmsg failed: " << strerror(errno);
}

static int recv_fd(int conn) {
  struct iovec iov;
  char buf[1];
  iov.iov_base = buf;
  iov.iov_len = sizeof(buf);

  struct msghdr msg;
  memset(&msg, 0, sizeof(msg));
  msg.msg_iov = &iov;
  msg.msg_iovlen = 1;

  char cmsgbuf[CMSG_SPACE(sizeof(int))];
  msg.msg_control = (caddr_t)cmsgbuf;
  msg.msg_controllen = sizeof(cmsgbuf);

  int len = recvmsg(conn, &msg, 0);
  if (len <= 0)
    Error() << "recvmsg failed: " << strerror(errno);

  struct cmsghdr *cmsg;
  cmsg = CMSG_FIRSTHDR(&msg);
  return *(int *)CMSG_DATA(cmsg);
}

bool resume_daemon(char **argv, int *code) {
  int conn = socket(AF_UNIX, SOCK_STREAM, 0);
  if (conn == -1)
    Error() << "socket failed: " << strerror(errno);

  std::string path = "/tmp/mold-" + compute_sha1(argv);

  struct sockaddr_un name;
  memset(&name, 0, sizeof(name));
  name.sun_family = AF_UNIX;
  memcpy(name.sun_path, path.data(), path.size());

  if (connect(conn, (struct sockaddr *)&name, sizeof(name)) != 0) {
    close(conn);
    return false;
  }

  send_fd(conn, STDOUT_FILENO);
  send_fd(conn, STDERR_FILENO);
  int r = read(conn, (char[1]){}, 1);
  *code = (r != 1);
  return true;
}

void daemonize(char **argv, std::function<void()> *wait_for_client,
               std::function<void()> *on_complete) {
  compute_sha1(argv);

  if (daemon(1, 0) == -1)
    Error() << "daemon failed: " << strerror(errno);

  int sock = socket(AF_UNIX, SOCK_STREAM, 0);
  if (sock == -1)
    Error() << "socket failed: " << strerror(errno);

  socket_tmpfile = strdup(("/tmp/mold-" + compute_sha1(argv)).c_str());

  struct sockaddr_un name;
  memset(&name, 0, sizeof(name));
  name.sun_family = AF_UNIX;
  strcpy(name.sun_path, socket_tmpfile);

  if (bind(sock, (struct sockaddr *)&name, sizeof(name)) == -1) {
    if (errno != EADDRINUSE)
      Error() << "bind failed: " << strerror(errno);

    unlink(socket_tmpfile);
    if (bind(sock, (struct sockaddr *)&name, sizeof(name)) == -1)
      Error() << "bind failed: " << strerror(errno);
  }

  if (listen(sock, 0) == -1)
    Error() << "listen failed: " << strerror(errno);

  static int conn = -1;

  *wait_for_client = [=]() {
    fd_set rfds;
    FD_ZERO(&rfds);
    FD_SET(sock, &rfds);

    struct timeval tv;
    tv.tv_sec = DAEMON_TIMEOUT;
    tv.tv_usec = 0;

    int res = select(sock + 1, &rfds, NULL, NULL, &tv);
    if (res == -1)
      Error() << "select failed: " << strerror(errno);

    if (res == 0) {
      std::cout << "timeout\n";
      exit(0);
    }

    conn = accept(sock, NULL, NULL);
    if (conn == -1)
      Error() << "accept failed: " << strerror(errno);
    unlink(socket_tmpfile);
  };

  *on_complete = [=]() { write(conn, (char []){1}, 1); };
}
