#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

static struct wl_compositor *compositor;
static struct wl_subcompositor *subcompositor;
static struct wl_shm *shm;
static struct xdg_wm_base *wm_base;
static bool configured;

static void reg_global(void *d, struct wl_registry *r, uint32_t name, const char *iface, uint32_t ver) {
  (void)d;
  if (!strcmp(iface, wl_compositor_interface.name)) compositor = wl_registry_bind(r, name, &wl_compositor_interface, 4);
  else if (!strcmp(iface, wl_subcompositor_interface.name)) subcompositor = wl_registry_bind(r, name, &wl_subcompositor_interface, 1);
  else if (!strcmp(iface, wl_shm_interface.name)) shm = wl_registry_bind(r, name, &wl_shm_interface, 1);
  else if (!strcmp(iface, xdg_wm_base_interface.name)) wm_base = wl_registry_bind(r, name, &xdg_wm_base_interface, ver < 2 ? ver : 2);
}
static void reg_remove(void *d, struct wl_registry *r, uint32_t n) { (void)d; (void)r; (void)n; }
static const struct wl_registry_listener reg_l = { reg_global, reg_remove };
static void ping(void *d, struct xdg_wm_base *b, uint32_t s) { (void)d; xdg_wm_base_pong(b, s); }
static const struct xdg_wm_base_listener wm_l = { ping };
static void surf_conf(void *d, struct xdg_surface *s, uint32_t serial) { (void)d; xdg_surface_ack_configure(s, serial); configured = true; }
static const struct xdg_surface_listener surf_l = { surf_conf };
static void top_conf(void *d, struct xdg_toplevel *t, int32_t w, int32_t h, struct wl_array *s) { (void)d; (void)t; (void)w; (void)h; (void)s; }
static void top_close(void *d, struct xdg_toplevel *t) { (void)d; (void)t; }
static const struct xdg_toplevel_listener top_l = { top_conf, top_close };

static struct wl_buffer *solid(int w, int h, uint32_t argb) {
  int stride = w * 4, size = stride * h;
  int fd = memfd_create("p0", 0);
  if (ftruncate(fd, size) != 0) { perror("ftruncate"); exit(1); }
  uint32_t *px = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  for (int i = 0; i < w * h; i++) px[i] = argb;
  munmap(px, size);
  struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, size);
  struct wl_buffer *b = wl_shm_pool_create_buffer(pool, 0, w, h, stride, WL_SHM_FORMAT_ARGB8888);
  wl_shm_pool_destroy(pool);
  close(fd);
  return b;
}

static double now_ms(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1e3 + t.tv_nsec / 1e6; }

int main(int argc, char **argv) {
  const char *mode = argc > 1 ? argv[1] : "destroy";
  struct wl_display *dpy = wl_display_connect(NULL);
  if (!dpy) { fprintf(stderr, "no wayland display\n"); return 1; }
  struct wl_registry *reg = wl_display_get_registry(dpy);
  wl_registry_add_listener(reg, &reg_l, NULL);
  wl_display_roundtrip(dpy);
  if (!compositor || !subcompositor || !shm || !wm_base) { fprintf(stderr, "missing globals\n"); return 1; }
  xdg_wm_base_add_listener(wm_base, &wm_l, NULL);

  struct wl_surface *parent = wl_compositor_create_surface(compositor);
  struct xdg_surface *xs = xdg_wm_base_get_xdg_surface(wm_base, parent);
  xdg_surface_add_listener(xs, &surf_l, NULL);
  struct xdg_toplevel *top = xdg_surface_get_toplevel(xs);
  xdg_toplevel_add_listener(top, &top_l, NULL);
  xdg_toplevel_set_title(top, "ferryx-p0-5-subsurface");
  xdg_toplevel_set_app_id(top, "ferryx-p0-5");
  wl_surface_commit(parent);
  while (!configured) wl_display_dispatch(dpy);

  struct wl_buffer *pb = solid(400, 300, 0xff202020);
  wl_surface_attach(parent, pb, 0, 0);
  wl_surface_damage_buffer(parent, 0, 0, 400, 300);
  wl_surface_commit(parent);

  struct wl_surface *child = wl_compositor_create_surface(compositor);
  struct wl_subsurface *sub = wl_subcompositor_get_subsurface(subcompositor, child, parent);
  wl_subsurface_set_position(sub, 50, 50);
  wl_subsurface_set_desync(sub);
  struct wl_region *empty = wl_compositor_create_region(compositor);
  wl_surface_set_input_region(child, empty);
  wl_region_destroy(empty);
  struct wl_buffer *cb = solid(200, 150, 0xffff00ff);
  wl_surface_attach(child, cb, 0, 0);
  wl_surface_damage_buffer(child, 0, 0, 200, 150);
  wl_surface_commit(child);
  wl_surface_commit(parent);
  wl_display_roundtrip(dpy);
  printf("MAPPED child_visible_magenta=1\n"); fflush(stdout);
  usleep(800 * 1000);

  double t0 = now_ms();
  if (!strcmp(mode, "destroy")) {
    wl_subsurface_destroy(sub);
    wl_surface_destroy(child);
  } else {
    wl_surface_attach(child, NULL, 0, 0);
    wl_surface_commit(child);
  }
  int flushed = wl_display_flush(dpy);
  double t_flush = now_ms() - t0;
  wl_display_roundtrip(dpy);
  double t_rt = now_ms() - t0;
  printf("DETACHED mode=%s flush_rc=%d same_step_ms=%.3f server_ack_ms=%.3f\n", mode, flushed, t_flush, t_rt); fflush(stdout);
  usleep(800 * 1000);
  printf("DONE\n"); fflush(stdout);
  xdg_toplevel_destroy(top); xdg_surface_destroy(xs); wl_surface_destroy(parent);
  wl_display_disconnect(dpy);
  return 0;
}
