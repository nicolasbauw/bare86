/* SPDX-License-Identifier: GPL-3.0-or-later */
/* Enveloppe minimale de libslirp pour Bare86 : configuration (réseau
 * 10.0.2.0/24 de QEMU), minuteries, attente poll(). Tout se passe dans le
 * thread réseau ; Rust ne voit que ces quelques fonctions. */

#include <libslirp.h>
#include <poll.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <time.h>

typedef void (*bare86_rx_fn)(void *user, const uint8_t *frame, size_t len);

struct timer {
    SlirpTimerCb cb;
    void *cb_opaque;
    int64_t expire_ms; /* -1 : inactive */
    struct timer *next;
};

struct bare86_net {
    Slirp *slirp;
    bare86_rx_fn rx;
    void *user;
    struct timer *timers;
    struct pollfd *fds;
    int nfds, cap;
};

static int64_t now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static slirp_ssize_t cb_send(const void *buf, size_t len, void *opaque)
{
    struct bare86_net *n = opaque;
    n->rx(n->user, buf, len);
    return (slirp_ssize_t)len;
}

static void cb_guest_error(const char *msg, void *opaque)
{
    (void)opaque;
    fprintf(stderr, "[net] %s\n", msg);
}

static int64_t cb_clock(void *opaque)
{
    (void)opaque;
    return now_ns();
}

static void *cb_timer_new(SlirpTimerCb cb, void *cb_opaque, void *opaque)
{
    struct bare86_net *n = opaque;
    struct timer *t = calloc(1, sizeof *t);
    if (!t)
        return NULL;
    t->cb = cb;
    t->cb_opaque = cb_opaque;
    t->expire_ms = -1;
    t->next = n->timers;
    n->timers = t;
    return t;
}

static void cb_timer_free(void *timer, void *opaque)
{
    struct bare86_net *n = opaque;
    for (struct timer **p = &n->timers; *p; p = &(*p)->next) {
        if (*p == timer) {
            *p = ((struct timer *)timer)->next;
            free(timer);
            return;
        }
    }
}

static void cb_timer_mod(void *timer, int64_t expire_ms, void *opaque)
{
    (void)opaque;
    ((struct timer *)timer)->expire_ms = expire_ms;
}

static void cb_register(slirp_os_socket fd, void *opaque)
{
    (void)fd;
    (void)opaque;
}

static void cb_notify(void *opaque)
{
    (void)opaque;
}

static const SlirpCb callbacks = {
    .send_packet = cb_send,
    .guest_error = cb_guest_error,
    .clock_get_ns = cb_clock,
    .timer_new = cb_timer_new,
    .timer_free = cb_timer_free,
    .timer_mod = cb_timer_mod,
    .notify = cb_notify,
    .register_poll_socket = cb_register,
    .unregister_poll_socket = cb_register,
};

struct bare86_net *bare86_net_new(bare86_rx_fn rx, void *user)
{
    struct bare86_net *n = calloc(1, sizeof *n);
    if (!n)
        return NULL;
    n->rx = rx;
    n->user = user;
    SlirpConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    /* Version 6 : rappels register_poll_socket (sinon libslirp appelle
     * register_poll_fd, laissé nul). */
    cfg.version = 6;
    cfg.in_enabled = true;
    inet_pton(AF_INET, "10.0.2.0", &cfg.vnetwork);
    inet_pton(AF_INET, "255.255.255.0", &cfg.vnetmask);
    inet_pton(AF_INET, "10.0.2.2", &cfg.vhost);
    inet_pton(AF_INET, "10.0.2.15", &cfg.vdhcp_start);
    inet_pton(AF_INET, "10.0.2.3", &cfg.vnameserver);
    cfg.vhostname = "bare86";
    n->slirp = slirp_new(&cfg, &callbacks, n);
    if (!n->slirp) {
        free(n);
        return NULL;
    }
    return n;
}

/* Trame émise par la carte de l'invité. */
void bare86_net_input(struct bare86_net *n, const uint8_t *frame, size_t len)
{
    slirp_input(n->slirp, frame, (int)len);
}

/* Redirection d'un port de l'hôte (127.0.0.1) vers l'invité (10.0.2.15). */
int bare86_net_hostfwd(struct bare86_net *n, int udp, int host_port, int guest_port)
{
    struct in_addr host, guest;
    inet_pton(AF_INET, "127.0.0.1", &host);
    inet_pton(AF_INET, "10.0.2.15", &guest);
    return slirp_add_hostfwd(n->slirp, udp, host, host_port, guest, guest_port);
}

static int add_poll(slirp_os_socket fd, int events, void *opaque)
{
    struct bare86_net *n = opaque;
    if (n->nfds == n->cap) {
        int cap = n->cap ? n->cap * 2 : 16;
        struct pollfd *fds = realloc(n->fds, (size_t)cap * sizeof *fds);
        if (!fds)
            return -1;
        n->fds = fds;
        n->cap = cap;
    }
    short ev = 0;
    if (events & SLIRP_POLL_IN)
        ev |= POLLIN;
    if (events & SLIRP_POLL_OUT)
        ev |= POLLOUT;
    if (events & SLIRP_POLL_PRI)
        ev |= POLLPRI;
    n->fds[n->nfds] = (struct pollfd){ .fd = fd, .events = ev };
    return n->nfds++;
}

static int get_revents(int idx, void *opaque)
{
    struct bare86_net *n = opaque;
    short r = n->fds[idx].revents;
    int ev = 0;
    if (r & POLLIN)
        ev |= SLIRP_POLL_IN;
    if (r & POLLOUT)
        ev |= SLIRP_POLL_OUT;
    if (r & POLLPRI)
        ev |= SLIRP_POLL_PRI;
    if (r & POLLERR)
        ev |= SLIRP_POLL_ERR;
    if (r & POLLHUP)
        ev |= SLIRP_POLL_HUP;
    return ev;
}

/* Un tour : attend les sockets de slirp et `wake_fd` (trames de l'invité)
 * au plus `max_ms`, traite les événements et les minuteries échues.
 * Rend 1 si `wake_fd` est lisible ou fermé. */
int bare86_net_poll(struct bare86_net *n, int wake_fd, int max_ms)
{
    uint32_t timeout = (uint32_t)max_ms;
    n->nfds = 0;
    int wake = add_poll(wake_fd, SLIRP_POLL_IN, n);
    slirp_pollfds_fill_socket(n->slirp, &timeout, add_poll, n);
    int64_t now = now_ns() / 1000000;
    for (struct timer *t = n->timers; t; t = t->next) {
        if (t->expire_ms >= 0) {
            int64_t left = t->expire_ms - now;
            if (left < 0)
                left = 0;
            if (left < (int64_t)timeout)
                timeout = (uint32_t)left;
        }
    }
    int r = poll(n->fds, (nfds_t)n->nfds, (int)timeout);
    slirp_pollfds_poll(n->slirp, r < 0, get_revents, n);
    now = now_ns() / 1000000;
    /* Une minuterie échue à la fois : son rappel peut créer ou libérer des
     * minuteries, la liste est reparcourue depuis le début. */
    for (int fired = 1; fired;) {
        fired = 0;
        for (struct timer *t = n->timers; t; t = t->next) {
            if (t->expire_ms >= 0 && t->expire_ms <= now) {
                t->expire_ms = -1;
                t->cb(t->cb_opaque);
                fired = 1;
                break;
            }
        }
    }
    return r > 0 && (n->fds[wake].revents & (POLLIN | POLLHUP));
}

void bare86_net_free(struct bare86_net *n)
{
    slirp_cleanup(n->slirp);
    while (n->timers) {
        struct timer *t = n->timers;
        n->timers = t->next;
        free(t);
    }
    free(n->fds);
    free(n);
}
