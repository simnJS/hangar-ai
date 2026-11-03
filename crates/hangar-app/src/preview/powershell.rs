//! PowerShell, which highlight.js coloured and bat's set of grammars (the one
//! `two-face` carries) does not have.
//!
//! The grammars of that set are sublime-syntax files compiled ahead of time,
//! and a PowerShell one is not among them. Writing one by hand and loading it
//! would need syntect's YAML loader, a crate more for one language, so this is
//! a small tokenizer instead: the comments (`#` and `<# #>`), the strings
//! (single, double and here-strings, which run over lines), `$variables`, the
//! numbers, the keywords and operators, `[types]`, `-Parameters` and
//! `Verb-Noun` cmdlets. It reads the text once and has no state beyond the
//! place in it, so a line it cannot make sense of is plain and the next is not
//! affected.

use super::highlight::{Class, Lines, Run};

/// Whether a file is PowerShell, by the language it is named after or its
/// extension.
pub fn is_file(path: &str, language: Option<&str>) -> bool {
    language == Some("powershell")
        || path
            .rsplit_once('.')
            .is_some_and(|(_, extension)| ["ps1", "psm1", "psd1"].contains(&extension))
}

/// Whether a fenced block of Markdown names PowerShell.
pub fn is_language(language: &str) -> bool {
    ["powershell", "ps1", "pwsh", "ps"].contains(&language.to_ascii_lowercase().as_str())
}

/// Words that PowerShell reads as statements, in any case.
const KEYWORDS: &[&str] = &[
    "begin",
    "break",
    "catch",
    "class",
    "continue",
    "data",
    "define",
    "do",
    "dynamicparam",
    "else",
    "elseif",
    "end",
    "enum",
    "exit",
    "filter",
    "finally",
    "for",
    "foreach",
    "from",
    "function",
    "if",
    "in",
    "inlinescript",
    "parallel",
    "param",
    "process",
    "return",
    "sequence",
    "switch",
    "throw",
    "trap",
    "try",
    "until",
    "using",
    "var",
    "while",
    "workflow",
    "$true",
    "$false",
    "$null",
    "hidden",
];

/// What follows a `-` for it to be an operator (`-eq`, `-and`), not a
/// parameter.
const OPERATORS: &[&str] = &[
    "eq",
    "ne",
    "gt",
    "ge",
    "lt",
    "le",
    "like",
    "notlike",
    "match",
    "notmatch",
    "contains",
    "notcontains",
    "in",
    "notin",
    "and",
    "or",
    "xor",
    "not",
    "is",
    "isnot",
    "as",
    "replace",
    "split",
    "join",
    "f",
    "band",
    "bor",
    "bxor",
    "bnot",
    "shl",
    "shr",
    "ceq",
    "cne",
    "cgt",
    "cge",
    "clt",
    "cle",
    "clike",
    "cmatch",
    "ieq",
    "ine",
];

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether a `#` here starts a comment: at the start of a token, not inside
/// one (`a#b` is a word).
fn starts_token(before: Option<char>) -> bool {
    before.is_none_or(|c| c.is_whitespace() || matches!(c, ';' | '(' | '{' | '}' | '|' | ',' | '&'))
}

/// The text as (end, class) pieces, in order, each from where the last ended.
fn tokens(text: &str) -> Vec<(usize, Class)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|(_, c)| *c);
    let byte = |i: usize| chars.get(i).map_or(text.len(), |(b, _)| *b);
    let mut out: Vec<(usize, Class)> = Vec::new();
    let mut put = |end: usize, class: Class| {
        if let Some(last) = out.last_mut()
            && last.1 == class
        {
            last.0 = end;
        } else {
            out.push((end, class));
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        let before = i.checked_sub(1).and_then(at);
        let start = i;
        let class = match c {
            '<' if at(i + 1) == Some('#') => {
                // A block comment, to `#>` or the end of the text.
                i += 2;
                while i < chars.len() && !(at(i) == Some('#') && at(i + 1) == Some('>')) {
                    i += 1;
                }
                i = (i + 2).min(chars.len());
                Class::Comment
            }
            '#' if starts_token(before) => {
                while i < chars.len() && at(i) != Some('\n') {
                    i += 1;
                }
                Class::Comment
            }
            '@' if matches!(at(i + 1), Some('"' | '\''))
                && matches!(at(i + 2), Some('\n' | '\r')) =>
            {
                // A here-string runs to a line that starts with the quote and
                // `@`.
                let quote = at(i + 1).unwrap_or('"');
                i += 2;
                loop {
                    while i < chars.len() && at(i) != Some('\n') {
                        i += 1;
                    }
                    if i >= chars.len() {
                        break;
                    }
                    i += 1;
                    if at(i) == Some(quote) && at(i + 1) == Some('@') {
                        i += 2;
                        break;
                    }
                }
                Class::String
            }
            '"' => {
                i += 1;
                while i < chars.len() {
                    match at(i) {
                        // A backtick escapes the next character, and a quote
                        // doubled is one.
                        Some('`') => i += 2,
                        Some('"') if at(i + 1) == Some('"') => i += 2,
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                i = i.min(chars.len());
                Class::String
            }
            '\'' => {
                i += 1;
                while i < chars.len() {
                    match at(i) {
                        Some('\'') if at(i + 1) == Some('\'') => i += 2,
                        Some('\'') => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                i = i.min(chars.len());
                Class::String
            }
            '$' => {
                i += 1;
                if at(i) == Some('{') {
                    while i < chars.len() && at(i) != Some('}') && at(i) != Some('\n') {
                        i += 1;
                    }
                    i = (i + 1).min(chars.len());
                } else {
                    // `$name`, `$env:PATH`, `$_`, `$?`, `$$`.
                    if matches!(at(i), Some('$' | '?' | '^' | '_'))
                        && !at(i + 1).is_some_and(word_char)
                    {
                        i += 1;
                    } else {
                        while i < chars.len()
                            && (at(i).is_some_and(word_char) || at(i) == Some(':'))
                        {
                            i += 1;
                        }
                    }
                }
                let name = text[byte(start)..byte(i)].to_ascii_lowercase();
                if KEYWORDS.contains(&name.as_str()) {
                    Class::Keyword
                } else {
                    Class::Attr
                }
            }
            '-' if at(i + 1).is_some_and(|c| c.is_alphabetic()) && starts_token(before) => {
                i += 1;
                while i < chars.len() && at(i).is_some_and(word_char) {
                    i += 1;
                }
                let name = text[byte(start + 1)..byte(i)].to_ascii_lowercase();
                if OPERATORS.contains(&name.as_str()) {
                    Class::Keyword
                } else {
                    Class::Attr
                }
            }
            '[' => {
                // A type: `[string]`, `[System.IO.Path]`, `[int[]]`.
                let mut j = i + 1;
                while j < chars.len()
                    && j - i < 60
                    && at(j).is_some_and(|c| word_char(c) || matches!(c, '.' | ','))
                {
                    j += 1;
                }
                while at(j) == Some('[') && at(j + 1) == Some(']') {
                    j += 2;
                }
                if j > i + 1 && at(j) == Some(']') && at(i + 1).is_some_and(char::is_alphabetic) {
                    i = j + 1;
                    Class::Title
                } else {
                    i += 1;
                    Class::Plain
                }
            }
            c if c.is_ascii_digit() && !before.is_some_and(|b| word_char(b) || b == '-') => {
                if c == '0' && matches!(at(i + 1), Some('x' | 'X')) {
                    i += 2;
                    while at(i).is_some_and(|c| c.is_ascii_hexdigit()) {
                        i += 1;
                    }
                } else {
                    while at(i).is_some_and(|c| c.is_ascii_digit()) {
                        i += 1;
                    }
                    if at(i) == Some('.') && at(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                        i += 1;
                        while at(i).is_some_and(|c| c.is_ascii_digit()) {
                            i += 1;
                        }
                    }
                    // KB, MB, GB, TB and PB are written after a number.
                    let suffix: String = [at(i), at(i + 1)]
                        .into_iter()
                        .flatten()
                        .collect::<String>()
                        .to_ascii_lowercase();
                    if ["kb", "mb", "gb", "tb", "pb"].contains(&suffix.as_str()) {
                        i += 2;
                    }
                }
                if at(i).is_some_and(word_char) {
                    // A word that starts with digits (`7zip`) is a word.
                    while at(i).is_some_and(|c| word_char(c) || c == '-') {
                        i += 1;
                    }
                    Class::Plain
                } else {
                    Class::Number
                }
            }
            c if word_char(c) => {
                while at(i).is_some_and(word_char) {
                    i += 1;
                }
                let first = text[byte(start)..byte(i)].to_ascii_lowercase();
                // `Verb-Noun`: a cmdlet is two words joined by a dash, the
                // first of which starts a capital.
                let cmdlet = at(i) == Some('-')
                    && at(i + 1).is_some_and(char::is_alphabetic)
                    && c.is_uppercase()
                    && starts_token(before);
                if cmdlet {
                    i += 1;
                    while at(i).is_some_and(word_char) {
                        i += 1;
                    }
                    Class::Title
                } else if KEYWORDS.contains(&first.as_str()) && !matches!(at(i), Some('-')) {
                    Class::Keyword
                } else {
                    Class::Plain
                }
            }
            _ => {
                i += 1;
                Class::Plain
            }
        };
        put(byte(i), class);
    }
    out
}

/// The text coloured, line by line: as many lines as it has line breaks plus
/// one.
pub fn highlight(text: &str) -> Lines {
    let mut lines: Lines = vec![Vec::new()];
    let mut from = 0;
    for (end, class) in tokens(text) {
        for (n, piece) in text[from..end].split('\n').enumerate() {
            if n > 0 {
                lines.push(Vec::new());
            }
            if piece.is_empty() {
                continue;
            }
            let line = lines.last_mut().expect("a line");
            match line.last_mut() {
                Some(last) if last.class == class => last.text.push_str(piece),
                _ => line.push(Run {
                    text: piece.to_string(),
                    class,
                }),
            }
        }
        from = end;
    }
    for line in &mut lines {
        if line.is_empty() {
            line.push(Run {
                text: String::new(),
                class: Class::Plain,
            });
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The (text, class) runs of the line `row`.
    fn runs(lines: &Lines, row: usize) -> Vec<(&str, Class)> {
        lines[row]
            .iter()
            .map(|run| (run.text.as_str(), run.class))
            .collect()
    }

    #[test]
    fn colours_a_statement_with_its_variables_cmdlets_and_strings() {
        let lines = highlight("if ($count -gt 3) { Write-Host 'big' }");
        let all = runs(&lines, 0);
        assert!(all.contains(&("if", Class::Keyword)), "{all:?}");
        assert!(all.contains(&("$count", Class::Attr)), "{all:?}");
        assert!(all.contains(&("-gt", Class::Keyword)), "{all:?}");
        assert!(all.contains(&("3", Class::Number)), "{all:?}");
        assert!(all.contains(&("Write-Host", Class::Title)), "{all:?}");
        assert!(all.contains(&("'big'", Class::String)), "{all:?}");
    }

    #[test]
    fn keeps_every_character_on_its_own_line() {
        let source = "# a\n<# one\ntwo #>\n$x = @\"\nhere $x\n\"@\nWrite-Host \"hi `\" there\"\n";
        let lines = highlight(source);
        assert_eq!(lines.len(), source.split('\n').count());
        let joined: Vec<String> = lines
            .iter()
            .map(|line| line.iter().map(|run| run.text.as_str()).collect())
            .collect();
        assert_eq!(joined.join("\n"), source);
    }

    #[test]
    fn a_block_comment_and_a_here_string_run_over_lines() {
        let lines = highlight("<# one\ntwo #> $a\n@'\nraw $text\n'@\n$b");
        assert_eq!(runs(&lines, 0), [("<# one", Class::Comment)]);
        assert_eq!(runs(&lines, 1)[0], ("two #>", Class::Comment));
        assert_eq!(runs(&lines, 2), [("@'", Class::String)]);
        assert_eq!(
            runs(&lines, 3),
            [("raw $text", Class::String)],
            "no variables in raw text"
        );
        assert_eq!(runs(&lines, 4), [("'@", Class::String)]);
        assert_eq!(runs(&lines, 5), [("$b", Class::Attr)]);
    }

    #[test]
    fn a_hash_starts_a_comment_only_at_the_start_of_a_token() {
        let lines = highlight("Write-Host a#b # real\nx;# after a semicolon");
        let first = runs(&lines, 0);
        assert!(first.contains(&("# real", Class::Comment)), "{first:?}");
        assert!(
            !first
                .iter()
                .any(|(text, class)| text.contains("a#b") && *class == Class::Comment)
        );
        assert!(runs(&lines, 1).contains(&("# after a semicolon", Class::Comment)));
    }

    #[test]
    fn strings_know_their_escapes() {
        let lines = highlight("\"a `\" b\" 'it''s' x");
        let row = runs(&lines, 0);
        assert_eq!(row[0], ("\"a `\" b\"", Class::String), "{row:?}");
        assert_eq!(row[2], ("'it''s'", Class::String), "{row:?}");
        assert_eq!(row[3], (" x", Class::Plain));
    }

    #[test]
    fn variables_numbers_parameters_and_types() {
        let lines = highlight(
            "Get-ChildItem -Path $env:TEMP -Depth 2 -Recurse [string[]]$names 0x1F 3.5mb 7zip",
        );
        let row = runs(&lines, 0);
        assert!(row.contains(&("Get-ChildItem", Class::Title)), "{row:?}");
        assert!(row.contains(&("-Path", Class::Attr)), "{row:?}");
        assert!(row.contains(&("$env:TEMP", Class::Attr)), "{row:?}");
        assert!(row.contains(&("2", Class::Number)), "{row:?}");
        assert!(row.contains(&("[string[]]", Class::Title)), "{row:?}");
        assert!(row.contains(&("0x1F", Class::Number)), "{row:?}");
        assert!(row.contains(&("3.5mb", Class::Number)), "{row:?}");
        assert!(
            !row.iter()
                .any(|(text, class)| text.contains("7zip") && *class == Class::Number),
            "a word that starts with digits is a word: {row:?}"
        );
    }

    #[test]
    fn keywords_are_found_in_any_case_and_only_as_whole_words() {
        let lines = highlight("FOREACH ($i in $list) { Return $i }\nforeachy returned $true");
        let row = runs(&lines, 0);
        assert!(row.contains(&("FOREACH", Class::Keyword)), "{row:?}");
        assert!(row.contains(&("Return", Class::Keyword)), "{row:?}");
        let second = runs(&lines, 1);
        assert!(
            !second
                .iter()
                .any(|(text, class)| *text == "foreachy" && *class == Class::Keyword)
        );
        assert!(second.contains(&("$true", Class::Keyword)), "{second:?}");
    }

    #[test]
    fn an_empty_text_and_an_unterminated_string_are_still_lines() {
        assert_eq!(highlight("").len(), 1);
        let open = highlight("'never closed\nnext");
        assert_eq!(open.len(), 2);
        assert_eq!(runs(&open, 0), [("'never closed", Class::String)]);
        let odd = highlight("[ $x [ ] ${a b} ``");
        assert_eq!(odd.len(), 1);
    }
}
