/* The login screen's side of greetd's protocol: see greetd.h. */
#define _GNU_SOURCE
#include "greetd.h"

#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

/* greetd's answers are small; anything longer is not greetd */
#define GREETD_MAX_MSG 65536

int greetd_json_escape(const char *in, char *out, size_t out_len) {
    size_t o = 0;
    for (const unsigned char *p = (const unsigned char *)in; *p; p++) {
        char tmp[8];
        const char *add = NULL;
        size_t n;
        switch (*p) {
            case '"': add = "\\\""; break;
            case '\\': add = "\\\\"; break;
            case '\n': add = "\\n"; break;
            case '\r': add = "\\r"; break;
            case '\t': add = "\\t"; break;
            default:
                if (*p < 0x20) {
                    snprintf(tmp, sizeof(tmp), "\\u%04x", *p);
                    add = tmp;
                }
        }
        if (add) {
            n = strlen(add);
            if (o + n >= out_len) return -1;
            memcpy(out + o, add, n);
            o += n;
        } else {
            if (o + 1 >= out_len) return -1;
            out[o++] = (char)*p;
        }
    }
    out[o] = '\0';
    return 0;
}

/* Reads a JSON string starting at *p (on the opening quote) into out (NULL: skip it); leaves *p past
 * the closing quote. Only the escapes greetd writes are decoded; \uXXXX below 0x80 too. */
static int read_string(const char **p, char *out, size_t out_len) {
    const char *s = *p;
    size_t o = 0;
    if (*s != '"') return -1;
    s++;
    while (*s && *s != '"') {
        char c = *s++;
        if (c == '\\') {
            char e = *s++;
            switch (e) {
                case '"': c = '"'; break;
                case '\\': c = '\\'; break;
                case '/': c = '/'; break;
                case 'n': c = '\n'; break;
                case 'r': c = '\r'; break;
                case 't': c = '\t'; break;
                case 'b': c = '\b'; break;
                case 'f': c = '\f'; break;
                case 'u': {
                    unsigned v = 0;
                    for (int i = 0; i < 4; i++) {
                        char h = *s++;
                        v <<= 4;
                        if (h >= '0' && h <= '9') v |= (unsigned)(h - '0');
                        else if (h >= 'a' && h <= 'f') v |= (unsigned)(h - 'a' + 10);
                        else if (h >= 'A' && h <= 'F') v |= (unsigned)(h - 'A' + 10);
                        else return -1;
                    }
                    c = v < 0x80 ? (char)v : '?';
                    break;
                }
                default: return -1;
            }
        }
        if (out) {
            if (o + 1 >= out_len) return -1;
            out[o++] = c;
        }
    }
    if (*s != '"') return -1;
    if (out) out[o] = '\0';
    *p = s + 1;
    return 0;
}

static const char *skip_ws(const char *p) {
    while (*p == ' ' || *p == '\n' || *p == '\r' || *p == '\t') p++;
    return p;
}

/* Skips one value that is not a string: null, true, false, a number. greetd sends nothing else in
 * the messages this screen reads. */
static int skip_scalar(const char **p) {
    const char *s = *p;
    while (*s && *s != ',' && *s != '}' && *s != ' ' && *s != '\n') s++;
    if (s == *p) return -1;
    *p = s;
    return 0;
}

int greetd_json_string(const char *json, const char *key, char *out, size_t out_len) {
    const char *p = skip_ws(json);
    char k[64];
    if (*p != '{') return -1;
    p = skip_ws(p + 1);
    while (*p && *p != '}') {
        if (read_string(&p, k, sizeof(k)) != 0) return -1;
        p = skip_ws(p);
        if (*p != ':') return -1;
        p = skip_ws(p + 1);
        if (*p == '"') {
            if (strcmp(k, key) == 0) return read_string(&p, out, out_len);
            if (read_string(&p, NULL, 0) != 0) return -1;
        } else {
            if (strcmp(k, key) == 0) return -1;
            if (skip_scalar(&p) != 0) return -1;
        }
        p = skip_ws(p);
        if (*p == ',') p = skip_ws(p + 1);
    }
    return -1;
}

static int send_msg(int fd, const char *json) {
    uint32_t len = (uint32_t)strlen(json);
    if (write(fd, &len, sizeof(len)) != (ssize_t)sizeof(len)) return -1;
    size_t done = 0;
    while (done < len) {
        ssize_t n = write(fd, json + done, len - done);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) return -1;
        done += (size_t)n;
    }
    return 0;
}

/* The next message from greetd, malloc'd and NUL-terminated (NULL: closed or not greetd). */
static char *recv_msg(int fd) {
    uint32_t len;
    size_t got = 0;
    while (got < sizeof(len)) {
        ssize_t n = read(fd, (char *)&len + got, sizeof(len) - got);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) return NULL;
        got += (size_t)n;
    }
    if (len == 0 || len > GREETD_MAX_MSG) return NULL;
    char *buf = malloc((size_t)len + 1);
    if (!buf) return NULL;
    got = 0;
    while (got < len) {
        ssize_t n = read(fd, buf + got, len - got);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { free(buf); return NULL; }
        got += (size_t)n;
    }
    buf[len] = '\0';
    return buf;
}

/* Ends the attempt and reads greetd's answer to it, so greetd does not find the socket closed under
 * its reply. */
static void cancel(int fd) {
    if (send_msg(fd, "{\"type\":\"cancel_session\"}") == 0) free(recv_msg(fd));
}

static void say(char *detail, size_t detail_len, const char *what) {
    if (detail && detail_len) snprintf(detail, detail_len, "%s", what);
}

GreetdResult greetd_login(const char *socket_path, const char *username, const char *password,
                          const char *session_cmd, char *detail, size_t detail_len) {
    if (!socket_path || !*socket_path) {
        say(detail, detail_len, "Not started by greetd (no GREETD_SOCK)");
        return GREETD_FAILED;
    }
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) {
        say(detail, detail_len, "Could not open a socket");
        return GREETD_FAILED;
    }
    struct sockaddr_un addr = {.sun_family = AF_UNIX};
    if (strlen(socket_path) >= sizeof(addr.sun_path)) {
        close(fd);
        say(detail, detail_len, "The greetd socket path is too long");
        return GREETD_FAILED;
    }
    strcpy(addr.sun_path, socket_path);
    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        close(fd);
        say(detail, detail_len, "Could not reach greetd");
        return GREETD_FAILED;
    }

    GreetdResult result = GREETD_FAILED;
    bool answered_secret = false, session_asked = false;
    /* sized for the escaped password: up to 6 bytes for each input byte */
    char *esc = malloc(6 * 256 + 512);
    char *req = malloc(6 * 256 + 1024);
    char type[32], sub[32];
    if (!esc || !req) { say(detail, detail_len, "Out of memory"); goto out; }

    if (greetd_json_escape(username, esc, 6 * 256 + 512) != 0) { say(detail, detail_len, "The username is too long"); goto out; }
    snprintf(req, 6 * 256 + 1024, "{\"type\":\"create_session\",\"username\":\"%s\"}", esc);
    if (send_msg(fd, req) != 0) { say(detail, detail_len, "greetd closed the connection"); goto out; }

    for (;;) {
        char *msg = recv_msg(fd);
        if (!msg) { say(detail, detail_len, "greetd closed the connection"); break; }
        if (greetd_json_string(msg, "type", type, sizeof(type)) != 0) {
            free(msg);
            say(detail, detail_len, "An answer from greetd this screen does not understand");
            break;
        }
        if (strcmp(type, "success") == 0) {
            free(msg);
            if (session_asked) { result = GREETD_OK; break; }
            /* signed in (no password was needed, or it was right): start the desktop */
            if (greetd_json_escape(session_cmd, esc, 6 * 256 + 512) != 0) { say(detail, detail_len, "The session command is too long"); break; }
            snprintf(req, 6 * 256 + 1024, "{\"type\":\"start_session\",\"cmd\":[\"/bin/sh\",\"-c\",\"%s\"],\"env\":[\"INFYNITAE_SIGNED_IN=1\"]}", esc);
            session_asked = true;
            if (send_msg(fd, req) != 0) { say(detail, detail_len, "greetd closed the connection"); break; }
            continue;
        }
        if (strcmp(type, "error") == 0) {
            char desc[160] = "";
            (void)greetd_json_string(msg, "error_type", sub, sizeof(sub));
            (void)greetd_json_string(msg, "description", desc, sizeof(desc));
            free(msg);
            result = strcmp(sub, "auth_error") == 0 ? GREETD_DENIED : GREETD_FAILED;
            if (result == GREETD_FAILED) say(detail, detail_len, desc[0] ? desc : "greetd refused");
            cancel(fd);
            break;
        }
        if (strcmp(type, "auth_message") == 0) {
            if (greetd_json_string(msg, "auth_message_type", sub, sizeof(sub)) != 0) sub[0] = '\0';
            free(msg);
            if (strcmp(sub, "secret") == 0) {
                if (answered_secret) {
                    say(detail, detail_len, "This account asks for more than a password");
                    cancel(fd);
                    break;
                }
                answered_secret = true;
                if (greetd_json_escape(password, esc, 6 * 256 + 512) != 0) {
                    cancel(fd);
                    say(detail, detail_len, "The password is too long");
                    break;
                }
                snprintf(req, 6 * 256 + 1024, "{\"type\":\"post_auth_message_response\",\"response\":\"%s\"}", esc);
                explicit_bzero(esc, 6 * 256 + 512);
                int rc = send_msg(fd, req);
                explicit_bzero(req, 6 * 256 + 1024);
                if (rc != 0) { say(detail, detail_len, "greetd closed the connection"); break; }
            } else if (strcmp(sub, "visible") == 0) {
                /* a prompt that echoes (PAM asking for the login name again): the name */
                if (greetd_json_escape(username, esc, 6 * 256 + 512) != 0) break;
                snprintf(req, 6 * 256 + 1024, "{\"type\":\"post_auth_message_response\",\"response\":\"%s\"}", esc);
                if (send_msg(fd, req) != 0) { say(detail, detail_len, "greetd closed the connection"); break; }
            } else {
                /* info and error lines need an empty answer to go on */
                if (send_msg(fd, "{\"type\":\"post_auth_message_response\",\"response\":null}") != 0) {
                    say(detail, detail_len, "greetd closed the connection");
                    break;
                }
            }
            continue;
        }
        free(msg);
        say(detail, detail_len, "An answer from greetd this screen does not understand");
        cancel(fd);
        break;
    }

out:
    if (esc) { explicit_bzero(esc, 6 * 256 + 512); free(esc); }
    if (req) { explicit_bzero(req, 6 * 256 + 1024); free(req); }
    close(fd);
    return result;
}
