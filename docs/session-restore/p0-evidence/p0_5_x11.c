#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <X11/Xlib.h>

static double now_ms(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1e3 + t.tv_nsec / 1e6; }

int main(int argc, char **argv) {
  const char *mode = argc > 1 ? argv[1] : "destroy";
  Display *d = XOpenDisplay(NULL);
  if (!d) { fprintf(stderr, "no X display\n"); return 1; }
  int s = DefaultScreen(d);
  Window parent = XCreateSimpleWindow(d, RootWindow(d, s), 0, 0, 400, 300, 0, 0, 0x202020);
  XStoreName(d, parent, "ferryx-p0-5-x11");
  XMapWindow(d, parent);
  Window child = XCreateSimpleWindow(d, parent, 50, 50, 200, 150, 0, 0, 0xff00ff);
  XMapWindow(d, child);
  XSync(d, False);
  usleep(800 * 1000);
  XWindowAttributes wa; XGetWindowAttributes(d, child, &wa);
  printf("MAPPED child_map_state=%d\n", wa.map_state);
  double t0 = now_ms();
  if (!strcmp(mode, "destroy")) XDestroyWindow(d, child); else XUnmapWindow(d, child);
  XFlush(d);
  double t_flush = now_ms() - t0;
  XSync(d, False);
  double t_sync = now_ms() - t0;
  int state = -1;
  if (strcmp(mode, "destroy")) { XGetWindowAttributes(d, child, &wa); state = wa.map_state; }
  Window root_ret, parent_ret, *kids = NULL; unsigned n = 0;
  XQueryTree(d, parent, &root_ret, &parent_ret, &kids, &n);
  if (kids) XFree(kids);
  printf("DETACHED mode=%s same_step_ms=%.3f server_ack_ms=%.3f child_map_state_after=%d parent_children_after=%u\n", mode, t_flush, t_sync, state, n);
  XDestroyWindow(d, parent);
  XCloseDisplay(d);
  return 0;
}
