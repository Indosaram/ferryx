#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <ghostty/vt.h>

#define CHECK(expr) do { GhosttyResult _r = (expr); if (_r != GHOSTTY_SUCCESS) { fprintf(stderr, "FAIL %s -> %d at %s:%d\n", #expr, (int)_r, __FILE__, __LINE__); exit(2); } } while (0)

typedef struct { uint8_t *p; size_t len, cap; } Buf;

static void put(Buf *b, const void *d, size_t n) {
  if (b->len + n > b->cap) { b->cap = (b->len + n) * 2 + 1024; b->p = realloc(b->p, b->cap); }
  memcpy(b->p + b->len, d, n); b->len += n;
}
static void put_u8(Buf *b, uint8_t v) { put(b, &v, 1); }
static void put_u16(Buf *b, uint16_t v) { uint8_t x[2] = { v >> 8, v }; put(b, x, 2); }
static void put_u32(Buf *b, uint32_t v) { uint8_t x[4] = { v >> 24, v >> 16, v >> 8, v }; put(b, x, 4); }
static void put_u64(Buf *b, uint64_t v) { put_u32(b, (uint32_t)(v >> 32)); put_u32(b, (uint32_t)v); }

static void put_color(Buf *b, GhosttyStyleColor c) {
  if (c.tag == GHOSTTY_STYLE_COLOR_PALETTE) { put_u8(b, 1); put_u8(b, c.value.palette); }
  else if (c.tag == GHOSTTY_STYLE_COLOR_RGB) { put_u8(b, 2); put_u8(b, c.value.rgb.r); put_u8(b, c.value.rgb.g); put_u8(b, c.value.rgb.b); }
  else put_u8(b, 0);
}

typedef struct { char **uris; size_t n, cap; } LinkTable;

static uint32_t link_id(LinkTable *t, const uint8_t *uri, size_t len) {
  for (size_t i = 0; i < t->n; i++) if (strlen(t->uris[i]) == len && memcmp(t->uris[i], uri, len) == 0) return (uint32_t)(i + 1);
  if (t->n == t->cap) { t->cap = t->cap ? t->cap * 2 : 16; t->uris = realloc(t->uris, t->cap * sizeof(char *)); }
  char *s = malloc(len + 1); memcpy(s, uri, len); s[len] = 0; t->uris[t->n++] = s;
  return (uint32_t)t->n;
}

static size_t g_cells, g_graphemes, g_links, g_styled;

static void encode_row(Buf *b, GhosttyTerminal term, GhosttyPointTag tag, uint32_t y, uint16_t cols, LinkTable *links) {
  GhosttyGridRef ref = GHOSTTY_INIT_SIZED(GhosttyGridRef);
  GhosttyPoint pt = { .tag = tag, .value = { .coordinate = { .x = 0, .y = y } } };
  CHECK(ghostty_terminal_grid_ref(term, pt, &ref));
  GhosttyRow row; CHECK(ghostty_grid_ref_row(&ref, &row));
  bool wrap = false; CHECK(ghostty_row_get(row, GHOSTTY_ROW_DATA_WRAP, &wrap));
  put_u8(b, wrap ? 1 : 0);
  put_u32(b, cols);
  for (uint16_t x = 0; x < cols; x++) {
    pt.value.coordinate.x = x;
    CHECK(ghostty_terminal_grid_ref(term, pt, &ref));
    GhosttyCell cell; CHECK(ghostty_grid_ref_cell(&ref, &cell));
    uint32_t cps[40]; size_t ncp = 0;
    GhosttyResult gr = ghostty_grid_ref_graphemes(&ref, cps, 40, &ncp);
    if (gr == GHOSTTY_OUT_OF_SPACE) { fprintf(stderr, "grapheme > 40 cps at %u,%u\n", x, y); exit(3); }
    CHECK(gr);
    uint32_t wide = 0; CHECK(ghostty_cell_get(cell, GHOSTTY_CELL_DATA_WIDE, &wide));
    put_u32(b, ncp ? cps[0] : 0);
    put_u8(b, ncp > 1 ? 1 : 0);
    if (ncp > 1) { g_graphemes++; put_u32(b, (uint32_t)(ncp - 1)); for (size_t i = 1; i < ncp; i++) put_u32(b, cps[i]); }
    put_u8(b, (uint8_t)wide);
    GhosttyStyle st = GHOSTTY_INIT_SIZED(GhosttyStyle);
    CHECK(ghostty_grid_ref_style(&ref, &st));
    if (!ghostty_style_is_default(&st)) g_styled++;
    put_color(b, st.fg_color); put_color(b, st.bg_color); put_color(b, st.underline_color);
    uint16_t attrs = (st.bold << 0) | (st.faint << 1) | (st.italic << 2) | ((st.underline != 0) << 3) | (st.blink << 6) | (st.inverse << 7) | (st.invisible << 8) | (st.strikethrough << 9) | (st.overline << 10);
    put_u16(b, attrs); put_u8(b, (uint8_t)st.underline);
    uint8_t ubuf[2048]; size_t ulen = 0;
    GhosttyResult hr = ghostty_grid_ref_hyperlink_uri(&ref, ubuf, sizeof ubuf, &ulen);
    if (hr == GHOSTTY_OUT_OF_SPACE) ulen = 0; else CHECK(hr);
    uint32_t lid = ulen ? link_id(links, ubuf, ulen) : 0;
    if (lid) g_links++;
    put_u32(b, lid);
    g_cells++;
  }
}

static Buf extract(GhosttyTerminal term, double *ms_out, size_t *sb_rows_out) {
  struct timespec t0, t1; clock_gettime(CLOCK_MONOTONIC, &t0);
  Buf b = {0}; LinkTable links = {0};
  uint16_t cols, rows; CHECK(ghostty_terminal_get(term, GHOSTTY_TERMINAL_DATA_COLS, &cols)); CHECK(ghostty_terminal_get(term, GHOSTTY_TERMINAL_DATA_ROWS, &rows));
  size_t sb = 0; CHECK(ghostty_terminal_get(term, GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS, &sb));
  put_u16(&b, cols); put_u16(&b, rows);
  for (uint32_t y = 0; y < rows; y++) encode_row(&b, term, GHOSTTY_POINT_TAG_ACTIVE, y, cols, &links);
  uint16_t cx, cy; bool cvis = false; CHECK(ghostty_terminal_get(term, GHOSTTY_TERMINAL_DATA_CURSOR_X, &cx)); CHECK(ghostty_terminal_get(term, GHOSTTY_TERMINAL_DATA_CURSOR_Y, &cy)); CHECK(ghostty_terminal_get(term, GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE, &cvis));
  put_u16(&b, cy); put_u16(&b, cx); put_u8(&b, cvis);
  GhosttyMode modes[] = { GHOSTTY_MODE_ALT_SCREEN_SAVE, GHOSTTY_MODE_DECCKM, GHOSTTY_MODE_KEYPAD_KEYS, GHOSTTY_MODE_BRACKETED_PASTE, GHOSTTY_MODE_FOCUS_EVENT, GHOSTTY_MODE_ORIGIN, GHOSTTY_MODE_WRAPAROUND, GHOSTTY_MODE_REVERSE_COLORS, GHOSTTY_MODE_INSERT, GHOSTTY_MODE_CURSOR_VISIBLE, GHOSTTY_MODE_SYNC_OUTPUT, GHOSTTY_MODE_NORMAL_MOUSE, GHOSTTY_MODE_BUTTON_MOUSE, GHOSTTY_MODE_ANY_MOUSE, GHOSTTY_MODE_SGR_MOUSE, GHOSTTY_MODE_CURSOR_BLINKING };
  uint32_t mflags = 0;
  for (size_t i = 0; i < sizeof modes / sizeof modes[0]; i++) { GhosttyTerminalModeConfig mc = { .mode = modes[i], .value = false }; CHECK(ghostty_terminal_get(term, GHOSTTY_TERMINAL_DATA_MODE, &mc)); if (mc.value) mflags |= 1u << i; }
  put_u32(&b, mflags);
  GhosttyColorRgb pal[256]; CHECK(ghostty_terminal_get(term, GHOSTTY_TERMINAL_DATA_COLOR_PALETTE, pal));
  for (int i = 0; i < 256; i++) { put_u8(&b, pal[i].r); put_u8(&b, pal[i].g); put_u8(&b, pal[i].b); }
  GhosttyString title = {0}; GhosttyResult tr = ghostty_terminal_get(term, GHOSTTY_TERMINAL_DATA_TITLE, &title);
  if (tr == GHOSTTY_SUCCESS) { put_u32(&b, (uint32_t)title.len); put(&b, title.ptr, title.len); } else put_u32(&b, 0);
  for (size_t y = 0; y < sb; y++) encode_row(&b, term, GHOSTTY_POINT_TAG_HISTORY, (uint32_t)y, cols, &links);
  put_u32(&b, (uint32_t)links.n);
  for (size_t i = 0; i < links.n; i++) { size_t l = strlen(links.uris[i]); put_u32(&b, (uint32_t)(i + 1)); put_u32(&b, (uint32_t)l); put(&b, links.uris[i], l); free(links.uris[i]); }
  free(links.uris);
  clock_gettime(CLOCK_MONOTONIC, &t1);
  *ms_out = (t1.tv_sec - t0.tv_sec) * 1e3 + (t1.tv_nsec - t0.tv_nsec) / 1e6;
  *sb_rows_out = sb;
  return b;
}


static uint64_t fnv(const Buf *b) { uint64_t h = 1469598103934665603ull; for (size_t i = 0; i < b->len; i++) { h ^= b->p[i]; h *= 1099511628211ull; } return h; }

static void feed(GhosttyTerminal t, const char *s) { ghostty_terminal_vt_write(t, (const uint8_t *)s, strlen(s)); }

static GhosttyTerminal mk(uint16_t c, uint16_t r, size_t sb_lines) {
  GhosttyTerminal t; CHECK(ghostty_terminal_new(NULL, &t, c, r));
  CHECK(ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES, NULL));
  size_t lines = sb_lines; CHECK(ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES, &lines));
  return t;
}

static uint64_t row_hash(GhosttyTerminal term, GhosttyPointTag tag, uint32_t y, uint16_t cols) {
  Buf b = {0}; LinkTable lt = {0};
  encode_row(&b, term, tag, y, cols, &lt);
  uint64_t h = fnv(&b);
  free(b.p);
  for (size_t i = 0; i < lt.n; i++) free(lt.uris[i]);
  free(lt.uris);
  return h;
}

typedef struct { uint64_t *h; size_t n; } Hist;

static Hist history(GhosttyTerminal t) {
  size_t sb = 0; CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS, &sb));
  uint16_t cols; CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_COLS, &cols));
  Hist h = { malloc((sb + 1) * sizeof(uint64_t)), sb };
  for (size_t y = 0; y < sb; y++) h.h[y] = row_hash(t, GHOSTTY_POINT_TAG_HISTORY, (uint32_t)y, cols);
  return h;
}

static bool append_only(const Hist *a, const Hist *b) {
  for (size_t k = 0; k <= a->n; k++) {
    size_t keep = a->n - k;
    if (keep > b->n) continue;
    if (keep && memcmp(a->h + k, b->h, keep * sizeof(uint64_t))) continue;
    return true;
  }
  return false;
}

typedef struct {
  GhosttyTrackedGridRef anchor;
  uint64_t anchor_hash;
  uint32_t anchor_y;
  size_t sb;
  uint16_t cols, rows;
  int screen;
  bool valid;
} Detector;

static void det_arm(Detector *d, GhosttyTerminal t) {
  CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS, &d->sb));
  CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_COLS, &d->cols));
  CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_ROWS, &d->rows));
  CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &d->screen));
  if (d->anchor) { ghostty_tracked_grid_ref_free(d->anchor); d->anchor = NULL; }
  d->valid = false;
  if (d->sb == 0) return;
  d->anchor_y = (uint32_t)(d->sb - 1);
  GhosttyPoint p = { .tag = GHOSTTY_POINT_TAG_HISTORY, .value = { .coordinate = { .x = 0, .y = d->anchor_y } } };
  CHECK(ghostty_terminal_grid_ref_track(t, p, &d->anchor));
  d->anchor_hash = row_hash(t, GHOSTTY_POINT_TAG_HISTORY, d->anchor_y, d->cols);
  d->valid = true;
}

static bool det_rewritten(Detector *d, GhosttyTerminal t) {
  size_t sb; uint16_t cols, rows; int screen;
  CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS, &sb));
  CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_COLS, &cols));
  CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_ROWS, &rows));
  CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &screen));
  if (cols != d->cols || rows != d->rows || screen != d->screen) return true;
  if (!d->valid) return false;
  if (!ghostty_tracked_grid_ref_has_value(d->anchor)) return sb != 0 || d->sb != 0;
  GhosttyPointCoordinate c = {0};
  if (ghostty_tracked_grid_ref_point(d->anchor, GHOSTTY_POINT_TAG_HISTORY, &c) != GHOSTTY_SUCCESS) return true;
  if (c.y > d->anchor_y || c.y >= sb) return true;
  uint32_t evicted = d->anchor_y - c.y;
  if (sb + evicted < d->sb) return true;
  return row_hash(t, GHOSTTY_POINT_TAG_HISTORY, c.y, cols) != d->anchor_hash;
}

static void load_fixture(GhosttyTerminal t, int lines) {
  char line[1024];
  for (int i = 0; i < lines; i++) {
    int n = snprintf(line, sizeof line, "\033[3%dm%05d \033[1;4:3;58;2;10;20;30mstyled\033[0m e\xcc\x81 \xf0\x9f\x91\xa9\xe2\x80\x8d\xf0\x9f\x92\xbb \xe4\xb8\xad\xe6\x96\x87 \033]8;;https://ex.com/%d\033\\link%d\033]8;;\033\\ %0*d\r\n", i % 8, i, i % 37, i % 37, 120, i);
    ghostty_terminal_vt_write(t, (const uint8_t *)line, (size_t)n);
  }
  feed(t, "\033]4;1;rgb:12/34/56\033\\\033]0;fixture-title\007\033[?2004h\033[?1h\033[?1006h\033[?1002h\033[5;10H");
}

int main(int argc, char **argv) {
  const char *mode = argc > 1 ? argv[1] : "extract";
  if (!strcmp(mode, "extract")) {
    GhosttyTerminal t = mk(200, 60, 10000);
    load_fixture(t, 10060);
    double ms; size_t sb; Buf a = extract(t, &ms, &sb);
    double ms2; size_t sb2; Buf b = extract(t, &ms2, &sb2);
    printf("EXTRACT cols=200 rows=60 scrollback_rows=%zu bytes=%zu ms=%.1f ms_repeat=%.1f deterministic=%s cells=%zu graphemes=%zu link_cells=%zu styled=%zu\n",
      sb, a.len, ms, ms2, (a.len == b.len && !memcmp(a.p, b.p, a.len)) ? "yes" : "NO", g_cells, g_graphemes, g_links, g_styled);
    GhosttyTerminal alt = mk(80, 24, 1000);
    feed(alt, "primary\r\n\033[?1049halt-screen \033[7minv\033[0m");
    double m3; size_t s3; Buf c = extract(alt, &m3, &s3);
    GhosttyTerminalModeConfig mc = { .mode = GHOSTTY_MODE_ALT_SCREEN_SAVE };
    CHECK(ghostty_terminal_get(alt, GHOSTTY_TERMINAL_DATA_MODE, &mc));
    int screen = -1; CHECK(ghostty_terminal_get(alt, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &screen));
    printf("ALT mode1049=%d active_screen=%d bytes=%zu\n", mc.value, screen, c.len);
    return 0;
  }
  if (!strcmp(mode, "rewrite")) {
    struct { const char *name; const char *seq; int resize_cols; } cases[] = {
      { "plain_output_scroll", "next-line\r\n", 0 },
      { "ED3_clear_scrollback", "\033[3J", 0 },
      { "resize_cols_reflow", NULL, 60 },
      { "resize_rows_only", NULL, -1 },
      { "RIS_reset", "\033c", 0 },
      { "alt_screen_enter", "\033[?1049h", 0 },
      { "DECSTBM_scroll_region_scroll", "\033[2;5r\033[5;1H\r\n\r\n\r\n\033[r", 0 },
    };
    for (size_t k = 0; k < sizeof cases / sizeof cases[0]; k++) {
      GhosttyTerminal t = mk(80, 10, 1000);
      for (int i = 0; i < 50; i++) { char l[96]; int n = snprintf(l, sizeof l, "row %02d %s\r\n", i, i % 3 == 0 ? "with a long tail that wraps when the terminal becomes narrower than eighty columns" : ""); ghostty_terminal_vt_write(t, (const uint8_t *)l, (size_t)n); }
      GhosttyTrackedGridRef first = NULL, mid = NULL;
      GhosttyPoint p0 = { .tag = GHOSTTY_POINT_TAG_HISTORY, .value = { .coordinate = { .x = 0, .y = 0 } } };
      GhosttyPoint p1 = { .tag = GHOSTTY_POINT_TAG_HISTORY, .value = { .coordinate = { .x = 0, .y = 20 } } };
      CHECK(ghostty_terminal_grid_ref_track(t, p0, &first)); CHECK(ghostty_terminal_grid_ref_track(t, p1, &mid));
      size_t sb_before = 0; CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS, &sb_before));
      double ms; size_t sbx; Buf before = extract(t, &ms, &sbx);
      uint64_t hist_before = 0; { Buf h = {0}; LinkTable lt = {0}; uint16_t c; ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_COLS, &c); for (size_t y = 0; y < sb_before; y++) encode_row(&h, t, GHOSTTY_POINT_TAG_HISTORY, (uint32_t)y, c, &lt); hist_before = fnv(&h); }
      if (cases[k].seq) feed(t, cases[k].seq);
      else if (cases[k].resize_cols > 0) CHECK(ghostty_terminal_resize(t, (uint16_t)cases[k].resize_cols, 10, 8, 16));
      else CHECK(ghostty_terminal_resize(t, 80, 14, 8, 16));
      size_t sb_after = 0; CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS, &sb_after));
      uint16_t cols_after; CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_COLS, &cols_after));
      Buf h = {0}; LinkTable lt = {0};
      size_t keep = sb_after < sb_before ? sb_after : sb_before;
      for (size_t y = 0; y < keep; y++) encode_row(&h, t, GHOSTTY_POINT_TAG_HISTORY, (uint32_t)y, cols_after, &lt);
      GhosttyPointCoordinate c0 = {0}, c1 = {0};
      bool v0 = ghostty_tracked_grid_ref_has_value(first), v1 = ghostty_tracked_grid_ref_has_value(mid);
      GhosttyResult r0 = v0 ? ghostty_tracked_grid_ref_point(first, GHOSTTY_POINT_TAG_HISTORY, &c0) : GHOSTTY_NO_VALUE;
      GhosttyResult r1 = v1 ? ghostty_tracked_grid_ref_point(mid, GHOSTTY_POINT_TAG_HISTORY, &c1) : GHOSTTY_NO_VALUE;
      printf("REWRITE case=%s sb_before=%zu sb_after=%zu cols_after=%u first_ref=%s(%d)@%u,%u mid_ref=%s(%d)@%u,%u history_prefix_hash_changed=%s\n",
        cases[k].name, sb_before, sb_after, cols_after,
        v0 ? "has" : "lost", (int)r0, c0.x, c0.y, v1 ? "has" : "lost", (int)r1, c1.x, c1.y,
        (keep == sb_before && fnv(&h) == hist_before) ? "no" : "yes");
      (void)before;
      ghostty_tracked_grid_ref_free(first); ghostty_tracked_grid_ref_free(mid);
      ghostty_terminal_free(t);
    }
    return 0;
  }
  if (!strcmp(mode, "detect-fuzz")) {
    unsigned seed = argc > 2 ? (unsigned)atoi(argv[2]) : 1;
    int runs = argc > 3 ? atoi(argv[3]) : 200;
    long steps = 0, truth_rw = 0, missed = 0, false_pos = 0;
    const char *ops[] = { "line\r\n", "\033[3J", "\033c", "\033[?1049h", "\033[?1049l", "\033[2J", "\033[1;1Hx", "\033[3;6r\033[6;1H\r\n\r\n\033[r", "\033[?47h", "\033[?47l", "\033[2K", "\033[10S", "\033[3T" };
    const int nops = sizeof ops / sizeof ops[0];
    for (int r = 0; r < runs; r++) {
      srand(seed * 7919u + (unsigned)r);
      GhosttyTerminal t = mk(40, 8, 60);
      Detector d = {0};
      for (int i = 0; i < 30; i++) { char l[64]; int n = snprintf(l, sizeof l, "init %d %.*s\r\n", i, rand() % 60, "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww"); ghostty_terminal_vt_write(t, (const uint8_t *)l, (size_t)n); }
      for (int s = 0; s < 150; s++) {
        Hist before = history(t);
        det_arm(&d, t);
        int k = rand() % 20;
        if (k < nops) feed(t, ops[k]);
        else if (k == nops) CHECK(ghostty_terminal_resize(t, (uint16_t)(20 + rand() % 40), 8, 8, 16));
        else if (k == nops + 1) CHECK(ghostty_terminal_resize(t, 40, (uint16_t)(4 + rand() % 12), 8, 16));
        else { char l[160]; int n = snprintf(l, sizeof l, "%d %.*s\r\n", s, rand() % 120, "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"); ghostty_terminal_vt_write(t, (const uint8_t *)l, (size_t)n); }
        Hist after = history(t);
        uint16_t cb = d.cols, rb = d.rows; int sc = d.screen;
        uint16_t ca, ra; int sa;
        CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_COLS, &ca)); CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_ROWS, &ra)); CHECK(ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &sa));
        bool truth = !append_only(&before, &after) || ca != cb || ra != rb || sa != sc;
        bool flagged = det_rewritten(&d, t);
        steps++; if (truth) truth_rw++;
        if (truth && !flagged) { missed++; if (missed <= 5) printf("MISS run=%d step=%d op=%d sb %zu->%zu\n", r, s, k, before.n, after.n); }
        if (!truth && flagged) false_pos++;
        free(before.h); free(after.h);
      }
      if (d.anchor) ghostty_tracked_grid_ref_free(d.anchor);
      ghostty_terminal_free(t);
    }
    printf("DETECT runs=%d steps=%ld truth_rewrites=%ld missed=%ld false_positives=%ld\n", runs, steps, truth_rw, missed, false_pos);
    return missed ? 4 : 0;
  }
  if (!strcmp(mode, "detect-cost")) {
    GhosttyTerminal t = mk(200, 60, 10000);
    load_fixture(t, 10060);
    Detector d = {0};
    struct timespec t0, t1; clock_gettime(CLOCK_MONOTONIC, &t0);
    int iters = 20000;
    for (int i = 0; i < iters; i++) { det_arm(&d, t); feed(t, "tick\r\n"); if (det_rewritten(&d, t)) { printf("unexpected rewrite at %d\n", i); return 5; } }
    clock_gettime(CLOCK_MONOTONIC, &t1);
    double us = ((t1.tv_sec - t0.tv_sec) * 1e9 + (t1.tv_nsec - t0.tv_nsec)) / 1e3 / iters;
    printf("DETECT_COST cols=200 per_step_us=%.2f (includes one vt_write of 6 bytes)\n", us);
    return 0;
  }
  fprintf(stderr, "usage: %s extract|rewrite|detect-fuzz|detect-cost\n", argv[0]);
  return 1;
}
