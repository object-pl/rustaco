//! Every language the editor knows about, with detection rules and editing configuration.
//! Available without the parsing engine, so the editor bundle can detect languages and
//! toggle comments before (or without) loading a grammar.

/// Editing data for a language, used by comment toggling, bracket matching and auto-closing.
#[derive(Clone, Copy, Debug, Default)]
pub struct LanguageConfig {
    pub line_comment: Option<&'static str>,
    pub block_comment: Option<(&'static str, &'static str)>,
    pub brackets: &'static [(&'static str, &'static str)],
    pub auto_closing: &'static [(&'static str, &'static str)],
}

#[derive(Clone, Copy, Debug)]
pub struct LanguageInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub file_names: &'static [&'static str],
    /// Interpreter names recognised in a `#!` first line.
    pub shebangs: &'static [&'static str],
    /// The language module that parses this language in the browser.
    pub pack: &'static str,
    pub config: LanguageConfig,
}

const BRACKETS: &[(&str, &str)] = &[("{", "}"), ("[", "]"), ("(", ")")];
const CLOSE_C: &[(&str, &str)] = &[("{", "}"), ("[", "]"), ("(", ")"), ("\"", "\""), ("'", "'")];
const CLOSE_JS: &[(&str, &str)] = &[("{", "}"), ("[", "]"), ("(", ")"), ("\"", "\""), ("'", "'"), ("`", "`")];
const CLOSE_RUST: &[(&str, &str)] = &[("{", "}"), ("[", "]"), ("(", ")"), ("\"", "\"")];
const CLOSE_TAGS: &[(&str, &str)] = &[("<", ">"), ("\"", "\""), ("'", "'")];

const C_STYLE: LanguageConfig = LanguageConfig {
    line_comment: Some("//"),
    block_comment: Some(("/*", "*/")),
    brackets: BRACKETS,
    auto_closing: CLOSE_C,
};
const HASH_STYLE: LanguageConfig =
    LanguageConfig { line_comment: Some("#"), block_comment: None, brackets: BRACKETS, auto_closing: CLOSE_C };

macro_rules! lang {
    ($id:literal, $name:literal, $pack:literal, ext: $ext:expr, files: $files:expr, shebangs: $sb:expr, $config:expr) => {
        LanguageInfo {
            id: $id,
            name: $name,
            pack: $pack,
            extensions: $ext,
            file_names: $files,
            shebangs: $sb,
            config: $config,
        }
    };
}

static CATALOG: &[LanguageInfo] = &[
    lang!("rust", "Rust", "rust", ext: &["rs"], files: &[], shebangs: &[],
        LanguageConfig { auto_closing: CLOSE_RUST, ..C_STYLE }),
    lang!("javascript", "JavaScript", "javascript", ext: &["js", "mjs", "cjs", "jsx"], files: &[],
        shebangs: &["node", "deno", "bun"], LanguageConfig { auto_closing: CLOSE_JS, ..C_STYLE }),
    lang!("typescript", "TypeScript", "typescript", ext: &["ts", "mts", "cts"], files: &[], shebangs: &["ts-node"],
        LanguageConfig { auto_closing: CLOSE_JS, ..C_STYLE }),
    lang!("tsx", "TSX", "typescript", ext: &["tsx"], files: &[], shebangs: &[],
        LanguageConfig { auto_closing: CLOSE_JS, ..C_STYLE }),
    lang!("json", "JSON", "json", ext: &["json", "jsonc", "json5"], files: &[".babelrc", ".eslintrc"], shebangs: &[],
    LanguageConfig {
        line_comment: None,
        block_comment: None,
        brackets: &[("{", "}"), ("[", "]")],
        auto_closing: &[("{", "}"), ("[", "]"), ("\"", "\"")],
    }),
    lang!("python", "Python", "python", ext: &["py", "pyi", "pyw"], files: &[], shebangs: &["python"],
        LanguageConfig { block_comment: Some(("\"\"\"", "\"\"\"")), ..HASH_STYLE }),
    lang!("c", "C", "c", ext: &["c", "h"], files: &[], shebangs: &[], C_STYLE),
    lang!("cpp", "C++", "cpp", ext: &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "ipp"], files: &[], shebangs: &[], C_STYLE),
    lang!("go", "Go", "go", ext: &["go"], files: &[], shebangs: &[], LanguageConfig { auto_closing: CLOSE_JS, ..C_STYLE }),
    lang!("java", "Java", "java", ext: &["java"], files: &[], shebangs: &[], C_STYLE),
    lang!("kotlin", "Kotlin", "kotlin", ext: &["kt", "kts"], files: &[], shebangs: &[], C_STYLE),
    lang!("csharp", "C#", "csharp", ext: &["cs", "csx"], files: &[], shebangs: &[], C_STYLE),
    lang!("html", "HTML", "html", ext: &["html", "htm", "xhtml"], files: &[], shebangs: &[],
        LanguageConfig { line_comment: None, block_comment: Some(("<!--", "-->")), brackets: &[("<", ">")], auto_closing: CLOSE_TAGS }),
    lang!("css", "CSS", "css", ext: &["css"], files: &[], shebangs: &[],
        LanguageConfig { line_comment: None, ..C_STYLE }),
    lang!("bash", "Shell", "bash", ext: &["sh", "bash", "zsh"], files: &[".bashrc", ".zshrc", ".profile"],
        shebangs: &["sh", "bash", "zsh", "dash"], HASH_STYLE),
    lang!("toml", "TOML", "toml", ext: &["toml"], files: &["Cargo.lock"], shebangs: &[], HASH_STYLE),
    lang!("yaml", "YAML", "yaml", ext: &["yaml", "yml"], files: &[], shebangs: &[], HASH_STYLE),
    lang!("sql", "SQL", "sql", ext: &["sql"], files: &[], shebangs: &[],
        LanguageConfig { line_comment: Some("--"), ..C_STYLE }),
    lang!("markdown", "Markdown", "markdown", ext: &["md", "markdown"], files: &[], shebangs: &[],
        LanguageConfig { line_comment: None, block_comment: Some(("<!--", "-->")), brackets: BRACKETS, auto_closing: CLOSE_C }),
    lang!("dockerfile", "Dockerfile", "dockerfile", ext: &["dockerfile"], files: &["Dockerfile", "Containerfile"],
        shebangs: &[], HASH_STYLE),
];

pub fn catalog() -> &'static [LanguageInfo] {
    CATALOG
}

pub fn info(id: &str) -> Option<&'static LanguageInfo> {
    CATALOG.iter().find(|l| l.id == id)
}

/// Picks a language from a file path and, when that fails, from the first line (`#!`).
pub fn detect(path: &str, first_line: &str) -> Option<&'static LanguageInfo> {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let by_name = CATALOG.iter().find(|l| l.file_names.contains(&file));
    let by_ext = || {
        let ext = file.rsplit_once('.')?.1.to_ascii_lowercase();
        CATALOG.iter().find(|l| l.extensions.contains(&ext.as_str()))
    };
    let by_prefix = || {
        // `Dockerfile.dev`, `Containerfile.prod`
        CATALOG.iter().find(|l| l.file_names.iter().any(|n| file.starts_with(n) && file[n.len()..].starts_with('.')))
    };
    let by_shebang = || {
        let line = first_line.strip_prefix("#!")?;
        let mut words = line.split_whitespace();
        let program = words.next()?.rsplit('/').next()?;
        let interpreter = if program == "env" { words.find(|w| !w.starts_with('-'))? } else { program };
        let base = interpreter.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
        CATALOG.iter().find(|l| l.shebangs.contains(&base))
    };
    by_name.or_else(by_prefix).or_else(by_ext).or_else(by_shebang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection() {
        let id = |p: &str, l: &str| detect(p, l).map(|i| i.id);
        assert_eq!(id("src/main.rs", ""), Some("rust"));
        assert_eq!(id("C:\\x\\App.JSX", ""), Some("javascript"));
        assert_eq!(id("script", "#!/usr/bin/env node"), Some("javascript"));
        assert_eq!(id("run", "#!/usr/bin/python3.12"), Some("python"));
        assert_eq!(id("deploy", "#!/bin/bash -e"), Some("bash"));
        assert_eq!(id(".babelrc", ""), Some("json"));
        assert_eq!(id("Dockerfile.dev", ""), Some("dockerfile"));
        assert_eq!(id("component.tsx", ""), Some("tsx"));
        assert_eq!(id("notes.txt", "hello"), None);
    }

    #[test]
    fn ids_are_unique() {
        for (i, a) in CATALOG.iter().enumerate() {
            assert!(CATALOG[i + 1..].iter().all(|b| b.id != a.id), "duplicate id {}", a.id);
        }
    }
}
