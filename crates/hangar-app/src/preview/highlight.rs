//! Syntax colouring for the preview's text, code and Markdown
//! (`components/preview/highlight.ts`).
//!
//! The TS used highlight.js with the 31 languages an agent's files are written
//! in, and the theme coloured the classes it produced (`.hljs-keyword` in the
//! accent, `.hljs-string` in the success tone…). Here the grammars are bat's,
//! through `two-face` (the set iced's own highlighter loads as well, which has
//! the TypeScript, TOML, Dockerfile, PowerShell and Kotlin that syntect's
//! default set lacks), run with `syntect`. iced's `Highlighter` hands over a
//! colour from one of five fixed syntect themes, which would not follow the
//! app's 33 themes nor stay readable on a light one; so the scopes are kept
//! and mapped to the same handful of classes the stylesheet coloured, and
//! `colour` picks each class's tone from the window's theme.
//!
//! Loading the grammars takes a moment and highlighting a 5,000-line file
//! longer: both belong on a blocking task, and the caller shows the text plain
//! until the colours arrive.

use std::sync::LazyLock;

use iced::Color;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use hangar_theme::Theme;

/// What a stretch of code is, in the terms of the classes the stylesheet
/// coloured (`.hljs-comment`, `.hljs-keyword`…).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Class {
    #[default]
    Plain,
    /// `comment`, `quote`: the quietest tone, in italics.
    Comment,
    /// `keyword`, `literal`, `selector-tag`, `doctag`: the accent.
    Keyword,
    /// `string`, `regexp`, `addition`, `template-tag`.
    String,
    /// `number`, `symbol`, `bullet`, `link`.
    Number,
    /// `title`, `section`, `name`, `type`, `built_in`, `selector-*`.
    Title,
    /// `attr`, `attribute`, `property`, `variable`, `params`.
    Attr,
    /// `meta`, `subst`.
    Meta,
    Deletion,
}

impl Class {
    pub fn italic(self) -> bool {
        self == Class::Comment
    }
}

/// The tone of a class in the window's theme, `None` for plain text, which
/// takes the colour of the text around it.
pub fn colour(theme: &Theme, class: Class) -> Option<Color> {
    let colors = &theme.colors;
    Some(match class {
        Class::Plain => return None,
        Class::Comment => colors.fg_subtle,
        Class::Keyword => colors.accent_hi,
        Class::String => colors.success,
        Class::Number => colors.warning,
        Class::Title => colors.state_yours,
        Class::Attr => colors.fg_secondary,
        Class::Meta => colors.fg_muted,
        Class::Deletion => colors.danger,
    })
}

/// A stretch of one line, in one class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub class: Class,
}

/// A highlighted text: its runs, line by line. As many lines as the text has
/// line breaks plus one, like `str::split('\n')`.
pub type Lines = Vec<Vec<Run>>;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// What a scope stands for, most specific first. A rule with no class stops
/// the search, so an operator does not take the colour of the string around
/// it; a scope with no rule at all is looked through (punctuation takes the
/// colour of what it punctuates).
static RULES: LazyLock<Vec<(Scope, Option<Class>)>> = LazyLock::new(|| {
    let rules: &[(&str, Option<Class>)] = &[
        // Deletions and additions of a diff before the strings and comments
        // they might be written in.
        ("markup.deleted", Some(Class::Deletion)),
        ("markup.inserted", Some(Class::String)),
        ("comment", Some(Class::Comment)),
        ("string.regexp", Some(Class::String)),
        ("string", Some(Class::String)),
        ("constant.numeric", Some(Class::Number)),
        ("constant.character.escape", Some(Class::Meta)),
        ("constant.language", Some(Class::Keyword)),
        ("constant.other.symbol", Some(Class::Number)),
        ("constant.other.color", Some(Class::Number)),
        ("markup.underline.link", Some(Class::Number)),
        ("markup.heading", Some(Class::Title)),
        ("markup.raw", Some(Class::String)),
        ("markup.list", Some(Class::Number)),
        ("markup.quote", Some(Class::Comment)),
        // Operators are not coloured by highlight.js.
        ("keyword.operator", None),
        ("keyword.control.directive", Some(Class::Meta)),
        ("keyword", Some(Class::Keyword)),
        ("storage", Some(Class::Keyword)),
        ("entity.name.tag", Some(Class::Title)),
        ("entity.other.attribute-name", Some(Class::Attr)),
        ("entity.other.inherited-class", Some(Class::Title)),
        ("entity.name.function.preprocessor", Some(Class::Meta)),
        ("entity.name.section", Some(Class::Title)),
        ("entity.name.function", Some(Class::Title)),
        ("entity.name.class", Some(Class::Title)),
        ("entity.name.type", Some(Class::Title)),
        ("entity.name.struct", Some(Class::Title)),
        ("entity.name.enum", Some(Class::Title)),
        ("entity.name.trait", Some(Class::Title)),
        ("entity.name.namespace", Some(Class::Title)),
        ("entity.name.selector", Some(Class::Title)),
        ("entity.name.label", Some(Class::Title)),
        ("entity.name.constant", Some(Class::Title)),
        ("support.type.property-name", Some(Class::Attr)),
        ("support.function", Some(Class::Title)),
        ("support.type", Some(Class::Title)),
        ("support.class", Some(Class::Title)),
        ("support.constant", Some(Class::Keyword)),
        ("meta.preprocessor", Some(Class::Meta)),
        ("meta.annotation", Some(Class::Meta)),
        ("meta.decorator", Some(Class::Meta)),
        ("variable.parameter", Some(Class::Attr)),
        ("variable.language", Some(Class::Attr)),
        ("variable.other.member", Some(Class::Attr)),
        ("variable.other.property", Some(Class::Attr)),
        ("variable.other.readwrite.alias", Some(Class::Plain)),
    ];
    rules
        .iter()
        .filter_map(|(scope, class)| Scope::new(scope).ok().map(|scope| (scope, *class)))
        .collect()
});

/// Scopes that make what is inside them a name rather than a value: the key of
/// a JSON object is a string to the grammar, an attribute to highlight.js.
static KEYS: LazyLock<Vec<Scope>> = LazyLock::new(|| {
    [
        "meta.mapping.key",
        "support.type.property-name",
        "entity.name.tag.yaml",
    ]
    .iter()
    .filter_map(|scope| Scope::new(scope).ok())
    .collect()
});

/// The class of a stack of scopes: the innermost scope a rule speaks for.
fn class_of(stack: &ScopeStack) -> Class {
    let scopes = stack.as_slice();
    if scopes
        .iter()
        .any(|scope| KEYS.iter().any(|key| key.is_prefix_of(*scope)))
    {
        return Class::Attr;
    }
    for scope in scopes.iter().rev() {
        for (prefix, class) in RULES.iter() {
            if prefix.is_prefix_of(*scope) {
                return class.unwrap_or(Class::Plain);
            }
        }
    }
    Class::Plain
}

/// The syntax of a file, by its name or extension first (so `Cargo.toml` is
/// TOML and a `.tsx` TSX, where highlight.js only had `ini` and
/// `typescript`), then by the language name the TS knew it by.
fn syntax_for(path: &str, language: Option<&str>) -> Option<&'static SyntaxReference> {
    let set: &'static SyntaxSet = &SYNTAXES;
    let name = path.rsplit(['/', '\\']).next().unwrap_or("");
    let by_file = || {
        // A leading dot is a hidden file, not an extension: `.env`, `.bashrc`.
        let extension = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty());
        set.find_syntax_by_extension(name)
            .or_else(|| {
                extension.and_then(|(_, extension)| set.find_syntax_by_extension(extension))
            })
            .filter(|syntax| syntax.name != "Plain Text")
    };
    by_file().or_else(|| language.and_then(syntax_of_language))
}

/// The syntax for a highlight.js language name (`languageOf`), or a fence's
/// info string in Markdown (`rust`, `ts`, `sh`).
pub fn syntax_of_language(language: &str) -> Option<&'static SyntaxReference> {
    let set: &'static SyntaxSet = &SYNTAXES;
    let token = match language {
        "bash" | "shell" => "sh",
        "csharp" => "cs",
        "dockerfile" => "Dockerfile",
        "dos" => "bat",
        "javascript" => "js",
        "kotlin" => "kt",
        "makefile" => "Makefile",
        "markdown" => "md",
        "powershell" => "ps1",
        "python" => "py",
        "ruby" => "rb",
        "rust" => "rs",
        "typescript" => "ts",
        "plaintext" | "text" | "txt" | "" => return None,
        other => other,
    };
    set.find_syntax_by_token(token)
        .or_else(|| set.find_syntax_by_extension(token))
        .or_else(|| set.find_syntax_by_name(token))
        .filter(|syntax| syntax.name != "Plain Text")
}

/// `text` coloured as the file `path` (and its `language`, as `language_of`
/// names it) is written; `None` for plain text, or a language no grammar is
/// known for, which the viewer draws as it is.
pub fn highlight_file(text: &str, path: &str, language: Option<&str>) -> Option<Lines> {
    let syntax = syntax_for(path, language)?;
    Some(run(text, syntax))
}

/// The same for a fenced block of Markdown, by its language word.
pub fn highlight_block(text: &str, language: &str) -> Option<Lines> {
    let language = language.split([' ', ',', '{']).next().unwrap_or("");
    let syntax = syntax_of_language(language)?;
    Some(run(text, syntax))
}

fn run(text: &str, syntax: &SyntaxReference) -> Lines {
    let set: &SyntaxSet = &SYNTAXES;
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut lines = Vec::new();
    for source in text.split('\n') {
        // The grammars end every line with a line break.
        let mut line = String::with_capacity(source.len() + 1);
        line.push_str(source);
        line.push('\n');
        let Ok(ops) = state.parse_line(&line, set) else {
            // A line the grammar cannot read is plain, and the next starts over.
            state = ParseState::new(syntax);
            stack = ScopeStack::new();
            lines.push(vec![Run {
                text: source.to_string(),
                class: Class::Plain,
            }]);
            continue;
        };
        let mut runs: Vec<Run> = Vec::new();
        let mut at = 0;
        let mut class = class_of(&stack);
        let push = |from: usize, to: usize, class: Class, runs: &mut Vec<Run>| {
            // The line break is not the text's.
            let to = to.min(source.len());
            if from >= to {
                return;
            }
            let piece = &source[from..to];
            match runs.last_mut() {
                Some(last) if last.class == class => last.text.push_str(piece),
                _ => runs.push(Run {
                    text: piece.to_string(),
                    class,
                }),
            }
        };
        for (position, op) in ops {
            push(at, position, class, &mut runs);
            at = at.max(position.min(source.len()));
            if stack.apply(&op).is_err() {
                stack = ScopeStack::new();
            }
            class = class_of(&stack);
        }
        push(at, source.len(), class, &mut runs);
        if runs.is_empty() {
            runs.push(Run {
                text: String::new(),
                class: Class::Plain,
            });
        }
        lines.push(runs);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::model::languages;

    fn classes(lines: &Lines, row: usize) -> Vec<(String, Class)> {
        lines[row]
            .iter()
            .map(|run| (run.text.clone(), run.class))
            .collect()
    }

    fn has(lines: &Lines, row: usize, text: &str, class: Class) -> bool {
        lines[row]
            .iter()
            .any(|run| run.text.contains(text) && run.class == class)
    }

    /// bat's set has no PowerShell, which highlight.js had: a `.ps1` is drawn
    /// plain, under its language chip.
    const WITHOUT_GRAMMAR: [&str; 1] = ["powershell"];

    #[test]
    fn knows_a_grammar_for_every_language_a_file_is_named_after() {
        let missing: Vec<&str> = languages()
            .into_iter()
            .filter(|language| syntax_of_language(language).is_none())
            .collect();
        assert_eq!(missing, WITHOUT_GRAMMAR);
    }

    #[test]
    fn highlights_every_line_of_a_file_a_block_comments_included() {
        let lines = highlight_file("/* a\n<b> */\nconst x = 1;", "x.ts", Some("typescript"))
            .expect("typescript has a grammar");
        assert_eq!(lines.len(), 3);
        assert!(
            has(&lines, 0, "/* a", Class::Comment),
            "{:?}",
            classes(&lines, 0)
        );
        // Not split in the middle of what it holds: the whole of the second
        // line is the comment's.
        assert_eq!(
            lines[1]
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "<b> */"
        );
        assert!(lines[1].iter().all(|run| run.class == Class::Comment));
        assert!(
            has(&lines, 2, "const", Class::Keyword),
            "{:?}",
            classes(&lines, 2)
        );
        assert!(
            has(&lines, 2, "1", Class::Number),
            "{:?}",
            classes(&lines, 2)
        );
    }

    #[test]
    fn leaves_plain_text_and_an_unknown_language_as_they_are() {
        assert!(highlight_file("plain", "notes.txt", None).is_none());
        assert!(highlight_file("plain", "LICENSE", None).is_none());
        assert!(highlight_block("plain", "klingon").is_none());
        assert!(highlight_block("plain", "").is_none());
    }

    #[test]
    fn keeps_every_character_of_the_text_on_its_own_line() {
        let source = "fn main() {\n    println!(\"héllo\");\n}\n\n// fin";
        let lines = highlight_file(source, "main.rs", Some("rust")).unwrap();
        let rebuilt: Vec<String> = lines
            .iter()
            .map(|runs| runs.iter().map(|run| run.text.as_str()).collect::<String>())
            .collect();
        assert_eq!(rebuilt.join("\n"), source);
    }

    #[test]
    fn colours_what_the_stylesheet_coloured() {
        let rust = highlight_file("let s = \"text\"; // note\n", "a.rs", Some("rust")).unwrap();
        assert!(
            has(&rust, 0, "let", Class::Keyword),
            "{:?}",
            classes(&rust, 0)
        );
        assert!(
            has(&rust, 0, "text", Class::String),
            "{:?}",
            classes(&rust, 0)
        );
        assert!(
            has(&rust, 0, "note", Class::Comment),
            "{:?}",
            classes(&rust, 0)
        );
        let json = highlight_file("{\"key\": 12, \"on\": true}", "a.json", Some("json")).unwrap();
        assert!(
            has(&json, 0, "12", Class::Number),
            "{:?}",
            classes(&json, 0)
        );
        assert!(
            has(&json, 0, "true", Class::Keyword),
            "{:?}",
            classes(&json, 0)
        );
    }

    #[test]
    fn a_file_is_known_by_its_extension_before_the_name_the_ts_gave_it() {
        // highlight.js had no TOML and called it ini.
        let toml =
            highlight_file("[package]\nname = \"hangar\"\n", "Cargo.toml", Some("ini")).unwrap();
        assert!(
            has(&toml, 1, "hangar", Class::String),
            "{:?}",
            classes(&toml, 1)
        );
        // And no Dockerfile by extension: by name.
        let docker = highlight_file(
            "FROM rust:1\nRUN cargo build\n",
            "Dockerfile",
            Some("dockerfile"),
        )
        .unwrap();
        assert!(
            has(&docker, 0, "FROM", Class::Keyword),
            "{:?}",
            classes(&docker, 0)
        );
    }

    #[test]
    fn the_names_of_a_mapping_are_attributes_and_its_values_are_what_they_are() {
        let json = highlight_file("{\"key\": \"text\"}", "a.json", Some("json")).unwrap();
        assert!(has(&json, 0, "key", Class::Attr), "{:?}", classes(&json, 0));
        assert!(has(&json, 0, "text", Class::String), "{:?}", classes(&json, 0));
        let yaml = highlight_file("name: hangar\n", "a.yml", Some("yaml")).unwrap();
        assert!(has(&yaml, 0, "name", Class::Attr), "{:?}", classes(&yaml, 0));
        let toml = highlight_file("name = \"hangar\"\n", "a.toml", Some("ini")).unwrap();
        assert!(has(&toml, 0, "hangar", Class::String), "{:?}", classes(&toml, 0));
    }

    #[test]
    fn a_fenced_block_is_found_by_the_word_after_its_fence() {
        let lines = highlight_block("const a = 1;", "ts").expect("ts");
        assert!(has(&lines, 0, "const", Class::Keyword));
        assert!(highlight_block("let a = 1;", "rust,ignore").is_some());
    }

    #[test]
    fn every_class_but_plain_has_a_tone_in_every_theme() {
        for theme in hangar_theme::themes() {
            assert_eq!(colour(theme, Class::Plain), None);
            for class in [
                Class::Comment,
                Class::Keyword,
                Class::String,
                Class::Number,
                Class::Title,
                Class::Attr,
                Class::Meta,
                Class::Deletion,
            ] {
                assert!(colour(theme, class).is_some(), "{class:?} in {}", theme.id);
            }
        }
    }
}
