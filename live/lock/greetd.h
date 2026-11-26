/*
 * The login screen's side of greetd's protocol (greetd-ipc(7)): a length-prefixed JSON message each
 * way over the socket in $GREETD_SOCK. greetd runs as root, checks the password through PAM, opens
 * the login session and starts the desktop as the user; the login screen only asks and answers.
 */
#ifndef INFYNITAE_GREETD_H
#define INFYNITAE_GREETD_H

#include <stddef.h>

typedef enum {
    GREETD_OK,          /* the session is started: greetd runs it once the login screen exits */
    GREETD_DENIED,      /* wrong user or password (auth_error) */
    GREETD_FAILED,      /* anything else: no socket, greetd's own error, an unexpected answer */
} GreetdResult;

/*
 * Logs `username` in with `password` and asks greetd to start `session_cmd` (run by /bin/sh -c as
 * the user). On GREETD_FAILED, `detail` says why in plain words. The password is answered to the
 * first secret prompt only and is never copied anywhere else; a second secret prompt (a second
 * factor this screen cannot ask for) ends the attempt.
 */
GreetdResult greetd_login(const char *socket_path, const char *username, const char *password,
                          const char *session_cmd, char *detail, size_t detail_len);

/* JSON helpers, exposed for the test: a string value of a key in a flat JSON object, unescaped
 * (returns 0, or -1 when the key or a string value is missing); a string escaped for JSON. */
int greetd_json_string(const char *json, const char *key, char *out, size_t out_len);
int greetd_json_escape(const char *in, char *out, size_t out_len);

#endif
