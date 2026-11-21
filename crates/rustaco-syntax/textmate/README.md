# Bundled TextMate grammars

These grammars feed the TextMate engine (`src/textmate.rs`) for languages without a tree-sitter grammar in the browser build. Each was converted once to JSON with Python's `plistlib` (Kotlin, SQL) or kept as JSON (Dockerfile); the content is otherwise unchanged.

| File | Language | Source | Commit | License |
|---|---|---|---|---|
| `sql.tmlanguage.json` | SQL | https://github.com/microsoft/vscode-mssql, `extensions/mssql/syntaxes/SQL.plist` | 684648ee5414eb2d9df22b0392ef05079f91e44e | MIT (`licenses/vscode-mssql.LICENSE`) |
| `kotlin.tmlanguage.json` | Kotlin | https://github.com/nishtahir/language-kotlin, `dist/Kotlin.tmLanguage` | 2a204bd8de2af687ea90fb0d6f8869afb07a0c28 | Apache-2.0 (`licenses/language-kotlin.LICENSE`, `licenses/language-kotlin.NOTICE`) |
| `docker.tmlanguage.json` | Dockerfile | https://github.com/microsoft/vscode, `extensions/docker/syntaxes/docker.tmLanguage.json`, taken from moby/moby | 8270a86019db7551da42b71d15c6080a414d8c81 | Apache-2.0 (`licenses/moby.LICENSE`, `licenses/moby.NOTICE`); VS Code's packaging MIT (`licenses/vscode.LICENSE`) |

Adding a grammar: put the JSON file here, add it to `GRAMMARS` in `src/textmate.rs`, set the language's `pack` to `"textmate"` in `src/catalog.rs`, and record it in this table and in THIRD_PARTY.md. Check `failed_regexes` with `cargo test -p rustaco-syntax --test textmate -- --nocapture`.
