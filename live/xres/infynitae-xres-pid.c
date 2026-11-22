/*
 * infynitae-xres-pid WINDOW... - the process that owns each X11 window, as the X server knows it
 *
 * Every X11 app reaches the compositor through xwayland-satellite's one Wayland connection, so
 * the compositor cannot see an X11 window's process. The X server can: X-Resource 1.2's
 * QueryClientIds gives the process id of the connection that created a window, which the server
 * read from the socket itself (SO_PEERCRED), so an app cannot lie about it the way it can with
 * _NET_WM_PID. Used by infynitae-xkill (Super+Shift+Delete on an X11 window).
 *
 * Prints "WINDOW PID" for each window given (as given, e.g. 0x1a00003), "WINDOW -" when the
 * server knows no process for it (no such window, a client over the network).
 *   exit 0: answered   2: no X display, or its server has no X-Resource 1.2   1: wrong use
 */

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>

#include <xcb/xcb.h>
#include <xcb/res.h>

/* the process of the client that created `window`, or 0 when the server knows none */
static uint32_t owner_pid(xcb_connection_t *c, uint32_t window)
{
    xcb_res_client_id_spec_t spec = {
        .client = window,
        .mask = XCB_RES_CLIENT_ID_MASK_LOCAL_CLIENT_PID,
    };
    xcb_res_query_client_ids_reply_t *reply =
        xcb_res_query_client_ids_reply(c, xcb_res_query_client_ids(c, 1, &spec), NULL);
    if (!reply)
        return 0;
    uint32_t pid = 0;
    for (xcb_res_client_id_value_iterator_t it = xcb_res_query_client_ids_ids_iterator(reply);
         it.rem; xcb_res_client_id_value_next(&it)) {
        if ((it.data->spec.mask & XCB_RES_CLIENT_ID_MASK_LOCAL_CLIENT_PID)
            && xcb_res_client_id_value_value_length(it.data) >= 1) {
            pid = xcb_res_client_id_value_value(it.data)[0];
            break;
        }
    }
    free(reply);
    return pid;
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: infynitae-xres-pid WINDOW...\n");
        return 1;
    }
    xcb_connection_t *c = xcb_connect(NULL, NULL);
    if (xcb_connection_has_error(c)) {
        fprintf(stderr, "infynitae-xres-pid: cannot open the X display\n");
        return 2;
    }
    const xcb_query_extension_reply_t *ext = xcb_get_extension_data(c, &xcb_res_id);
    xcb_res_query_version_reply_t *version = ext && ext->present
        ? xcb_res_query_version_reply(c, xcb_res_query_version(c, 1, 2), NULL)
        : NULL;
    int has_ids = version
        && (version->server_major > 1 || (version->server_major == 1 && version->server_minor >= 2));
    free(version);
    if (!has_ids) {
        fprintf(stderr, "infynitae-xres-pid: the X server has no X-Resource 1.2\n");
        xcb_disconnect(c);
        return 2;
    }
    for (int i = 1; i < argc; i++) {
        char *end;
        unsigned long window = strtoul(argv[i], &end, 0);
        uint32_t pid = (*argv[i] && !*end && window <= UINT32_MAX) ? owner_pid(c, window) : 0;
        if (pid)
            printf("%s %u\n", argv[i], pid);
        else
            printf("%s -\n", argv[i]);
    }
    xcb_disconnect(c);
    return 0;
}
