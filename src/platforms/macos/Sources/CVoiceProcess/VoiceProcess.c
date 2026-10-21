#include "VoiceProcess.h"
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

struct ambient_voice_child {
    pthread_mutex_t mutex;
    pid_t pid;
    int writer;
    int exited;
    int status;
};

static int write_all(int fd, const uint8_t *bytes, size_t size) {
    while (size) {
        ssize_t count = write(fd, bytes, size);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return -1;
        bytes += count;
        size -= (size_t)count;
    }
    return 0;
}

int32_t ambient_voice_child_wait(ambient_voice_child *child) {
    if (!child) return -1;
    siginfo_t info;
    int result;
    // WNOWAIT retains the zombie PID until the sole reaper takes the signaling mutex.
    do { result = waitid(P_PID, (id_t)child->pid, &info, WEXITED | WNOWAIT); }
    while (result < 0 && errno == EINTR);
    if (result < 0) return -1;
    pthread_mutex_lock(&child->mutex);
    int status = 0;
    pid_t waited;
    do { waited = waitpid(child->pid, &status, 0); } while (waited < 0 && errno == EINTR);
    if (waited != child->pid) {
        pthread_mutex_unlock(&child->mutex);
        return -1;
    }
    child->exited = 1;
    child->status = WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
    close(child->writer);
    child->writer = -1;
    pthread_mutex_unlock(&child->mutex);
    return child->status;
}

int32_t ambient_voice_child_signal(ambient_voice_child *child, int32_t signal_number) {
    if (!child || (signal_number != SIGTERM && signal_number != SIGKILL)) return -1;
    pthread_mutex_lock(&child->mutex);
    // A signal cannot race past reaping and reach a recycled PID.
    int result = child->exited ? 0 : kill(child->pid, signal_number);
    pthread_mutex_unlock(&child->mutex);
    return result;
}

void ambient_voice_child_destroy(ambient_voice_child *child) {
    if (!child) return;
    // The Swift owner retains this handle until its one wait has actually returned.
    if (!child->exited) return;
    pthread_mutex_destroy(&child->mutex);
    free(child);
}

ambient_voice_child *ambient_voice_child_spawn(const char *executable, const char *temporary_directory,
                                               const uint8_t *request, size_t count) {
    if (!executable || !temporary_directory || !request || count == 0 || count > 8192 ||
        strlen(executable) > 4096 || strlen(temporary_directory) > 4096) return NULL;
    int pipe_fds[2];
    if (pipe(pipe_fds) != 0) return NULL;
    int null_fd = open("/dev/null", O_RDWR | O_CLOEXEC);
    if (null_fd < 0) { close(pipe_fds[0]); close(pipe_fds[1]); return NULL; }
    if (fcntl(pipe_fds[0], F_SETFD, FD_CLOEXEC) != 0 ||
        fcntl(pipe_fds[1], F_SETFD, FD_CLOEXEC) != 0 ||
        fcntl(pipe_fds[1], F_SETFL, O_NONBLOCK) != 0 ||
        fcntl(pipe_fds[1], F_SETNOSIGPIPE, 1) != 0) {
        close(null_fd); close(pipe_fds[0]); close(pipe_fds[1]); return NULL;
    }
    ambient_voice_child *child = calloc(1, sizeof(*child));
    if (!child) { close(null_fd); close(pipe_fds[0]); close(pipe_fds[1]); return NULL; }
    if (pthread_mutex_init(&child->mutex, NULL) != 0) {
        free(child); close(null_fd); close(pipe_fds[0]); close(pipe_fds[1]); return NULL;
    }
    char *temporary_env = NULL;
    if (asprintf(&temporary_env, "TMPDIR=%s/", temporary_directory) < 0) {
        pthread_mutex_destroy(&child->mutex); free(child);
        close(null_fd); close(pipe_fds[0]); close(pipe_fds[1]); return NULL;
    }
    posix_spawn_file_actions_t actions;
    posix_spawnattr_t attributes;
    int actions_ready = posix_spawn_file_actions_init(&actions) == 0;
    int attributes_ready = posix_spawnattr_init(&attributes) == 0;
    int error = !actions_ready || !attributes_ready;
    if (!error) error = posix_spawn_file_actions_adddup2(&actions, pipe_fds[0], STDIN_FILENO);
    if (!error) error = posix_spawn_file_actions_adddup2(&actions, null_fd, STDOUT_FILENO);
    if (!error) error = posix_spawn_file_actions_adddup2(&actions, null_fd, STDERR_FILENO);
    if (!error) error = posix_spawnattr_setflags(&attributes, POSIX_SPAWN_CLOEXEC_DEFAULT);
    char *arguments[] = {(char *)executable, NULL};
    char *environment[] = {temporary_env, NULL};
    if (!error) error = posix_spawn(&child->pid, executable, &actions, &attributes, arguments, environment);
    if (actions_ready) posix_spawn_file_actions_destroy(&actions);
    if (attributes_ready) posix_spawnattr_destroy(&attributes);
    free(temporary_env);
    close(null_fd);
    close(pipe_fds[0]);
    if (error) {
        close(pipe_fds[1]); pthread_mutex_destroy(&child->mutex); free(child); return NULL;
    }
    child->writer = pipe_fds[1];
    uint8_t prefix[4] = {(uint8_t)(count >> 24), (uint8_t)(count >> 16),
                         (uint8_t)(count >> 8), (uint8_t)count};
    if (write_all(child->writer, prefix, sizeof(prefix)) || write_all(child->writer, request, count)) {
        ambient_voice_child_signal(child, SIGKILL);
        ambient_voice_child_wait(child);
        ambient_voice_child_destroy(child);
        return NULL;
    }
    return child;
}

static int read_all(int fd, uint8_t *bytes, size_t size) {
    while (size) {
        ssize_t count = read(fd, bytes, size);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return -1;
        bytes += count; size -= (size_t)count;
    }
    return 0;
}

int32_t ambient_voice_read_request(uint8_t *bytes, size_t capacity) {
    uint8_t prefix[4];
    if (!bytes || read_all(STDIN_FILENO, prefix, sizeof(prefix))) return -1;
    size_t count = (size_t)prefix[0] << 24 | (size_t)prefix[1] << 16 |
                   (size_t)prefix[2] << 8 | (size_t)prefix[3];
    if (!count || count > 8192 || count > capacity || read_all(STDIN_FILENO, bytes, count)) return -1;
    return (int32_t)count;
}

static void *watch_parent(void *context) {
    (void)context;
    uint8_t byte;
    ssize_t count;
    do { count = read(STDIN_FILENO, &byte, 1); } while (count < 0 && errno == EINTR);
    // No cooperative executor or CoreML callback is needed to release the helper's lease.
    _exit(125);
}

int32_t ambient_voice_watch_parent(void) {
    pthread_t thread;
    if (pthread_create(&thread, NULL, watch_parent, NULL) != 0) return -1;
    return pthread_detach(thread);
}
