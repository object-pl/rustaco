#ifndef TREE_SITTER_WASM_CTYPE_H_
#define TREE_SITTER_WASM_CTYPE_H_

typedef void *locale_t;

#ifndef weak_alias
#define weak_alias(old, new) \
  extern __typeof(old) new __attribute__((__weak__, __alias__(#old)))
#endif

int isblank(int c);

static inline int isprint(int c) {
  return c >= 0x20 && c <= 0x7E;
}

/* Rustaco patch: ASCII classifiers used by grammar scanners (Bash calls isdigit). */
static inline int isdigit(int c) {
  return c >= '0' && c <= '9';
}

static inline int isspace(int c) {
  return c == ' ' || (c >= 9 && c <= 13); /* tab, newline, vertical tab, form feed, carriage return */
}

static inline int isalpha(int c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
}

static inline int isalnum(int c) {
  return isalpha(c) || isdigit(c);
}

static inline int isxdigit(int c) {
  return isdigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
}

static inline int isupper(int c) {
  return c >= 'A' && c <= 'Z';
}

static inline int islower(int c) {
  return c >= 'a' && c <= 'z';
}

static inline int toupper(int c) {
  return islower(c) ? c - 32 : c;
}

static inline int tolower(int c) {
  return isupper(c) ? c + 32 : c;
}

#endif // TREE_SITTER_WASM_CTYPE_H_
