# Sourced by build scripts: compiler settings for C code (tree-sitter grammars) on wasm32.
# clang comes from PATH or from a portable LLVM in ~/tools; the libc subset tree-sitter
# needs on wasm32-unknown-unknown ships in the tree-sitter-language crate.

# Paths handed to cargo must be native; on Windows (Git Bash) convert /c/... to C:/...
native_path() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

if [ -z "$CC_wasm32_unknown_unknown" ]; then
  if command -v clang >/dev/null 2>&1; then
    CC_wasm32_unknown_unknown=$(native_path "$(command -v clang)")
  else
    for c in "$HOME"/tools/clang+llvm-*/bin/clang.exe "$HOME"/tools/clang+llvm-*/bin/clang; do
      [ -f "$c" ] && CC_wasm32_unknown_unknown=$(native_path "$c")
    done
  fi
fi
if [ -z "$CC_wasm32_unknown_unknown" ]; then
  echo "clang not found: install LLVM or unpack it into ~/tools" >&2
  return 1 2>/dev/null || exit 1
fi
export CC_wasm32_unknown_unknown

llvm_bin=$(dirname "$CC_wasm32_unknown_unknown")
if [ -f "$llvm_bin/llvm-ar.exe" ]; then
  AR_wasm32_unknown_unknown="$llvm_bin/llvm-ar.exe"
elif [ -f "$llvm_bin/llvm-ar" ]; then
  AR_wasm32_unknown_unknown="$llvm_bin/llvm-ar"
else
  AR_wasm32_unknown_unknown=llvm-ar
fi
export AR_wasm32_unknown_unknown

# cargo metadata prints JSON; Windows paths arrive with escaped backslashes.
ts_language_dir=$(cargo metadata --format-version 1 2>/dev/null \
  | grep -o '"manifest_path":"[^"]*tree-sitter-language[^"/\\]*[/\\]*Cargo.toml"' | head -1 \
  | sed 's/"manifest_path":"//; s/Cargo.toml"$//' | tr -s '\\' '/')
if [ -z "$ts_language_dir" ]; then
  echo "tree-sitter-language not found in cargo metadata" >&2
  return 1 2>/dev/null || exit 1
fi
export CFLAGS_wasm32_unknown_unknown="-I${ts_language_dir}wasm/include"
