//! Text for the TeamSpeak chat.
//!
//! The chat renders BBCode (`[b]`, `[url=...]`) in TS3 and TS6, and TS6 also renders Markdown.
//! Text from outside the bot (YouTube titles, words the user typed) goes through
//! `escape` before it is put into a reply, so it can never format or link anything.

use std::borrow::Cow;

/// Placed after a backslash that stands right before a bracket, see `escape`.
const ZERO_WIDTH_SPACE: char = '\u{200B}';

/// Makes `[` and `]` plain text by putting `\` before them, checked by hand in TS3 and TS6:
/// the backslash is not shown, tags (also unclosed ones) and Markdown links `[text](url)`
/// stay plain text. Markdown emphasis (`**`, `_`) is left alone: it cannot link anywhere,
/// and a backslash before it shows up in TS3.
///
/// A backslash already right before a bracket (`x\[b]`) gets an invisible space after it,
/// so it cannot cancel the escaping backslash. Text without brackets is returned as it is.
pub fn escape(text: &str) -> Cow<'_, str> {
    if !text.contains(['[', ']']) {
        return Cow::Borrowed(text);
    }
    let mut escaped = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' | ']' => {
                escaped.push('\\');
                escaped.push(c);
            }
            '\\' if matches!(chars.peek(), Some('[' | ']')) => {
                escaped.push(c);
                escaped.push(ZERO_WIDTH_SPACE);
            }
            _ => escaped.push(c),
        }
    }
    Cow::Owned(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_without_brackets_is_borrowed() {
        for text in ["Daft Punk - Get Lucky", "", "AC\\DC **bold** :/", "zolc"] {
            assert!(matches!(escape(text), Cow::Borrowed(_)), "{text}");
            assert_eq!(escape(text), text);
        }
    }

    #[test]
    fn brackets_get_a_backslash() {
        let cases = [
            ("[b]x[/b]", r"\[b\]x\[/b\]"),
            (
                "[url=https://evil.example]Lyrics[/url]",
                r"\[url=https://evil.example\]Lyrics\[/url\]",
            ),
            (
                "[link](https://evil.example)",
                r"\[link\](https://evil.example)",
            ),
            ("Song [Official Video]", r"Song \[Official Video\]"),
            ("unclosed [b", r"unclosed \[b"),
            ("]", r"\]"),
        ];
        for (text, expected) in cases {
            assert_eq!(escape(text), expected, "{text}");
        }
    }

    #[test]
    fn backslash_before_a_bracket_cannot_cancel_the_escape() {
        assert_eq!(escape(r"x\[b]"), "x\\\u{200B}\\[b\\]");
        assert_eq!(escape(r"\\[b]"), "\\\\\u{200B}\\[b\\]");
    }

    #[test]
    fn other_backslashes_stay() {
        assert_eq!(escape(r"C:\music\[x]"), "C:\\music\\\u{200B}\\[x\\]");
        assert_eq!(escape(r"a\b [c]"), r"a\b \[c\]");
    }

    #[test]
    fn multi_byte_text_is_kept() {
        assert_eq!(escape("Kult [żółć] 🎵"), r"Kult \[żółć\] 🎵");
    }

    #[test]
    fn every_bracket_is_escaped_and_nothing_else_changes() {
        let text = "a[b]c[[d]]e\\f";
        let escaped = escape(text);
        let unescaped = escaped
            .replace(ZERO_WIDTH_SPACE, "")
            .replace("\\[", "[")
            .replace("\\]", "]");
        assert_eq!(unescaped, text);
        assert_eq!(escaped.matches("\\[").count(), text.matches('[').count());
        assert_eq!(escaped.matches("\\]").count(), text.matches(']').count());
    }
}
