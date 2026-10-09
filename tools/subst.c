/* subst: replace literal text in a file, in place.
 *
 *   subst FILE OLD NEW
 *
 * Every occurrence of OLD in FILE becomes NEW; OLD and NEW are literal
 * text (no patterns) in which \n, \t and \\ stand for a newline, a tab and
 * a backslash. A line-anchored edit writes the newlines it means: OLD
 * "\nfoo\n" is the whole line "foo". The file is rewritten through a
 * temporary beside it and renamed, so a failed run leaves it as it was.
 * Exit 0 with "subst: FILE: N replaced" on standard error, 1 when OLD does
 * not occur (an upstream change the caller has to look at, never skipped
 * silently), 2 on a usage or system error. See subst.md. */
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

static char *unescape(const char *s, size_t *len) {
    char *o = malloc(strlen(s) + 1), *p = o;
    if (!o) return NULL;
    for (; *s; s++) {
        if (*s != '\\' || !s[1]) { *p++ = *s; continue; }
        switch (*++s) {
        case 'n': *p++ = '\n'; break;
        case 't': *p++ = '\t'; break;
        case '\\': *p++ = '\\'; break;
        default: *p++ = '\\'; *p++ = *s; break;   /* any other escape stays as written */
        }
    }
    *len = (size_t) (p - o);
    return o;
}

static char *slurp(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    size_t cap = 1 << 16, n = 0;
    char *b = malloc(cap);
    for (size_t r; b && (r = fread(b + n, 1, cap - n, f)) > 0;) {
        n += r;
        if (n == cap) {
            char *nb = realloc(b, cap *= 2);
            if (!nb) { free(b); b = NULL; }
            else b = nb;
        }
    }
    int bad = ferror(f);
    fclose(f);
    if (!b || bad) { free(b); return NULL; }
    *len = n;
    return b;
}

static char *memfind(char *h, size_t hl, const char *n, size_t nl) {
    for (size_t i = 0; nl <= hl && i <= hl - nl; i++)
        if (h[i] == n[0] && !memcmp(h + i, n, nl)) return h + i;
    return NULL;
}

int main(int argc, char **argv) {
    if (argc != 4 || !argv[2][0]) {
        fprintf(stderr, "usage: subst FILE OLD NEW   (literal text; \\n \\t \\\\ escapes; see subst.md)\n");
        return 2;
    }
    const char *path = argv[1];
    size_t ol, nl, len;
    char *old = unescape(argv[2], &ol), *new = unescape(argv[3], &nl);
    char *buf = slurp(path, &len);
    if (!old || !new || !buf) {
        fprintf(stderr, "subst: %s: %s\n", path, strerror(errno ? errno : ENOMEM));
        return 2;
    }
    char tmp[4096];
    if (snprintf(tmp, sizeof tmp, "%s.subst", path) >= (int) sizeof tmp) {
        fprintf(stderr, "subst: %s: path too long\n", path);
        return 2;
    }
    FILE *out = NULL;
    size_t count = 0;
    char *at = buf, *end = buf + len, *hit;
    while ((hit = memfind(at, (size_t) (end - at), old, ol))) {
        if (!out && !(out = fopen(tmp, "wb"))) {
            fprintf(stderr, "subst: %s: %s\n", tmp, strerror(errno));
            return 2;
        }
        fwrite(at, 1, (size_t) (hit - at), out);
        fwrite(new, 1, nl, out);
        at = hit + ol;
        count++;
    }
    if (!count) {
        fprintf(stderr, "subst: %s: the text to replace is not there (upstream changed?)\n", path);
        return 1;
    }
    fwrite(at, 1, (size_t) (end - at), out);
    struct stat st;
    if (ferror(out) | fclose(out) || stat(path, &st) || chmod(tmp, st.st_mode & 07777) || rename(tmp, path)) {
        fprintf(stderr, "subst: %s: %s\n", path, strerror(errno));
        remove(tmp);
        return 2;
    }
    fprintf(stderr, "subst: %s: %zu replaced\n", path, count);
    return 0;
}
