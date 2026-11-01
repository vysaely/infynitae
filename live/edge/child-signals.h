/* The edge starts apps and never waits for them, so the kernel should clean up after them.
 * signal(SIGCHLD, SIG_IGN) does that too, but an ignored signal stays ignored across exec: every
 * app would start with SIGCHLD ignored, and its own waitpid() would fail with ECHILD (Mocktail
 * quits with "No child processes"). SA_NOCLDWAIT on the default action cleans up the same way,
 * and exec clears it, so apps start with a normal SIGCHLD. */
#ifndef INFYNITAE_EDGE_CHILD_SIGNALS_H
#define INFYNITAE_EDGE_CHILD_SIGNALS_H

#include <signal.h>
#include <string.h>

static inline void reap_children_without_waiting(void) {
    struct sigaction action;
    memset(&action, 0, sizeof action);
    action.sa_handler = SIG_DFL;
    action.sa_flags = SA_NOCLDWAIT;
    sigemptyset(&action.sa_mask);
    sigaction(SIGCHLD, &action, NULL);
}

#endif
