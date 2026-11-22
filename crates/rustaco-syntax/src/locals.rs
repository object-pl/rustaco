//! Names within one file, from the syntax tree, for every tree-sitter language at once:
//! the keywords of the grammar, the identifiers of the file, where a name is declared and
//! which uses belong to that declaration.
//!
//! No grammar-specific queries are involved. A declaration is an identifier in the `name`,
//! `pattern` or `declarator` field of its parent (`left` too in assignments), or inside a
//! parameter or a pattern; its scope is the nearest enclosing block, function, class or
//! file. Shadowing inside a scope is not tracked: every use of the name in the
//! declaration's scope counts.

use std::collections::HashMap;
use std::ops::Range;

use rustaco_core::Rope;
use tree_sitter::{Language, Node, Tree};

/// Files larger than this get no name features: the walk over the whole tree would cost
/// more than a keystroke can wait.
const MAX_BYTES: usize = 4 << 20;

/// Node kinds that are names.
fn is_name(node: Node) -> bool {
    let kind = node.kind();
    node.is_named()
        && node.child_count() == 0
        && (kind.ends_with("identifier") || matches!(kind, "name" | "variable_name" | "simple_identifier"))
}

/// Node kinds that open a scope.
fn is_scope(kind: &str) -> bool {
    const PARTS: &[&str] = &[
        "block",
        "body",
        "function",
        "method",
        "closure",
        "lambda",
        "arrow",
        "class",
        "impl",
        "trait",
        "interface",
        "module",
        "namespace",
        "program",
        "source_file",
        "translation_unit",
        "compilation_unit",
        "document",
        "for_statement",
        "for_expression",
        "for_in_statement",
        "match_arm",
        "catch",
        "lexical_scope",
    ];
    PARTS.iter().any(|p| kind.contains(p))
}

/// The keywords of a grammar: its anonymous tokens that are words.
pub fn keywords(language: &Language) -> Vec<String> {
    let mut out: Vec<String> = (0..language.node_kind_count() as u16)
        .filter(|&id| !language.node_kind_is_named(id) && language.node_kind_is_visible(id))
        .filter_map(|id| language.node_kind_for_id(id))
        .filter(|k| {
            k.len() >= 2
                && k.starts_with(|c: char| c.is_ascii_alphabetic())
                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .map(str::to_owned)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The text of `node`.
fn text(rope: &Rope, node: Node) -> String {
    rope.byte_slice(node.start_byte()..node.end_byte()).to_string()
}

/// Every name in `root`, in document order.
fn names<'t>(root: Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if is_name(node) {
            out.push(node);
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() || cursor.node() == root {
                return out;
            }
        }
    }
}

/// The name at `byte` or just before it (a caret at the end of a word).
fn name_at(tree: &Tree, byte: usize) -> Option<Node<'_>> {
    let root = tree.root_node();
    let at = |b: usize| root.descendant_for_byte_range(b, b).filter(|n| is_name(*n));
    at(byte).or_else(|| byte.checked_sub(1).and_then(at))
}

/// True when `node` is declared where it stands.
fn is_declaration(node: Node) -> bool {
    let Some(parent) = node.parent() else { return false };
    let kind = parent.kind();
    let field = |f: &str| parent.child_by_field_name(f) == Some(node);
    field("name")
        || field("pattern")
        || field("declarator")
        // `left` is also the left operand of `x + 1`: only assignments declare.
        || (field("left") && (kind.contains("assignment") || kind.contains("declaration")))
        || kind.contains("parameter")
        || kind.contains("declarator")
        || kind.contains("pattern")
        || (kind == "expression_list" && parent.parent().is_some_and(|g| g.kind().contains("declaration")))
}

/// The scope a declaration is visible in.
fn scope_of(declaration: Node<'_>) -> Node<'_> {
    let parent = declaration.parent();
    // A parameter belongs to its function; anything else to the scope around the
    // declaring node (a function's name is visible next to the function, not only in it).
    let mut node = match parent {
        Some(p) if p.kind().contains("parameter") => Some(p),
        Some(p) => p.parent(),
        None => None,
    };
    let mut last = declaration;
    while let Some(n) = node {
        if is_scope(n.kind()) {
            return n;
        }
        last = n;
        node = n.parent();
    }
    last
}

fn contains(outer: Node, inner: Node) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

/// Where the name at `byte` is declared: of the declarations of that name whose scope holds
/// the use, the one in the innermost scope (the last before the use when a scope declares
/// it twice). Without one, the first occurrence of the name in the file.
pub fn definition(tree: &Tree, rope: &Rope, byte: usize) -> Option<Range<usize>> {
    if rope.len_bytes() > MAX_BYTES {
        return None;
    }
    let target = name_at(tree, byte)?;
    let name = text(rope, target);
    let all: Vec<Node> = names(tree.root_node()).into_iter().filter(|n| text(rope, *n) == name).collect();
    let mut best: Option<(Node, usize)> = None;
    for n in all.iter().copied().filter(|n| is_declaration(*n)) {
        let scope = scope_of(n);
        if !contains(scope, target) {
            continue;
        }
        let size = scope.end_byte() - scope.start_byte();
        let better = match best {
            None => true,
            Some((b, s)) => {
                size < s || (size == s && n.start_byte() <= target.start_byte() && n.start_byte() > b.start_byte())
            }
        };
        if better {
            best = Some((n, size));
        }
    }
    let found = best.map(|(n, _)| n).or_else(|| all.first().copied())?;
    Some(found.start_byte()..found.end_byte())
}

/// The uses of the name at `byte`: every occurrence in the scope of its declaration (the
/// whole file when no declaration is found), as byte ranges in document order.
pub fn references(tree: &Tree, rope: &Rope, byte: usize) -> Vec<Range<usize>> {
    if rope.len_bytes() > MAX_BYTES {
        return Vec::new();
    }
    let Some(target) = name_at(tree, byte) else { return Vec::new() };
    let name = text(rope, target);
    let root = tree.root_node();
    let scope = definition(tree, rope, byte)
        .and_then(|d| root.descendant_for_byte_range(d.start, d.end))
        .filter(|d| is_name(*d) && is_declaration(*d))
        .map_or(root, scope_of);
    names(scope).into_iter().filter(|n| text(rope, *n) == name).map(|n| n.start_byte()..n.end_byte()).collect()
}

/// True for a name the grammars accept as an identifier: a letter, `_` or `$`, then also
/// digits.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Names of the file for completion at `byte`, each once, nearest first: (name, distance in
/// bytes to its nearest occurrence). The word being typed at `byte` is left out unless it
/// also occurs elsewhere.
pub fn completion_names(tree: &Tree, rope: &Rope, byte: usize) -> Vec<(String, usize)> {
    if rope.len_bytes() > MAX_BYTES {
        return Vec::new();
    }
    let typing = name_at(tree, byte).filter(|n| n.end_byte() == byte).map(|n| n.start_byte());
    let mut nearest: HashMap<String, usize> = HashMap::new();
    for n in names(tree.root_node()) {
        if Some(n.start_byte()) == typing {
            continue;
        }
        let distance = if n.end_byte() <= byte { byte - n.end_byte() } else { n.start_byte().saturating_sub(byte) };
        let entry = nearest.entry(text(rope, n)).or_insert(usize::MAX);
        *entry = (*entry).min(distance);
    }
    let mut out: Vec<(String, usize)> = nearest.into_iter().collect();
    out.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    out
}

#[cfg(all(test, feature = "rust", feature = "python", feature = "javascript"))]
mod tests {
    use super::*;
    use crate::Syntax;

    fn parse(language: &str, source: &str) -> (Syntax, Rope) {
        let rope = Rope::from_str(source);
        let mut s = Syntax::new(crate::by_id(language).unwrap()).unwrap();
        s.parse(&rope);
        (s, rope)
    }

    fn at(source: &str, needle: &str, nth: usize) -> usize {
        source.match_indices(needle).nth(nth).unwrap().0
    }

    fn texts(source: &str, ranges: &[Range<usize>]) -> Vec<String> {
        ranges.iter().map(|r| source[r.clone()].to_owned()).collect()
    }

    #[test]
    fn rust_locals_stay_in_their_function() {
        let src = "fn one(x: i32) -> i32 { let y = x + 1; y * x }\nfn two(x: i32) -> i32 { x }\n";
        let (s, rope) = parse("rust", src);
        let tree = s.tree().unwrap();
        // `x` in `y * x` is the parameter of `one`.
        let use_ = at(src, "x }", 0);
        assert_eq!(definition(tree, &rope, use_), Some(7..8));
        let refs = references(tree, &rope, use_);
        assert_eq!(refs.len(), 3, "{:?}", texts(src, &refs));
        assert!(refs.iter().all(|r| r.end < src.find("fn two").unwrap()));
        // `two` is visible in the file: its declaration and nothing else.
        assert_eq!(references(tree, &rope, at(src, "two", 0)).len(), 1);
    }

    #[test]
    fn python_and_javascript() {
        let src = "def area(r):\n    pi = 3.14\n    return pi * r * r\n\nprint(area(2))\n";
        let (s, rope) = parse("python", src);
        let tree = s.tree().unwrap();
        assert_eq!(definition(tree, &rope, at(src, "area(2)", 0)), Some(4..8));
        assert_eq!(references(tree, &rope, at(src, "r * r", 0)).len(), 3);
        let src = "const total = 1;\nfunction add(n) { return total + n; }\nadd(total);\n";
        let (s, rope) = parse("javascript", src);
        let tree = s.tree().unwrap();
        let refs = references(tree, &rope, at(src, "total", 2));
        assert_eq!(refs.len(), 3);
        assert_eq!(definition(tree, &rope, at(src, "n;", 0)), Some(at(src, "n)", 0)..at(src, "n)", 0) + 1));
    }

    #[test]
    fn keywords_and_names_for_completion() {
        let (s, rope) = parse("rust", "fn main() { let alpha = 1; let beta = al }");
        let kws = keywords(&s.language().grammar());
        for k in ["fn", "let", "match", "impl", "struct"] {
            assert!(kws.iter().any(|w| w == k), "{k} missing");
        }
        let src = rope.to_string();
        let names = completion_names(s.tree().unwrap(), &rope, at(&src, "al }", 0) + 2);
        let list: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
        assert!(list.contains(&"alpha") && list.contains(&"beta") && list.contains(&"main"));
        assert!(!list.contains(&"al"));
        assert!(valid_name("snake_case") && valid_name("$el") && !valid_name("1x") && !valid_name("a-b"));
    }
}
