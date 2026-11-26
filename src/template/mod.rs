//! Placeholders in the texts of events (`message`, `description`, `nickname`), parsed when the
//! config is read.
//!
//! `{name}` made of lowercase letters and `_` is a placeholder, an unknown name is an error, so
//! a typo stops the bot at start like an unknown key. Other braces are plain text. Which
//! placeholder fits which event and field is checked by the config validation.

#[cfg(test)]
mod tests;

use std::{borrow::Cow, mem, str::FromStr};

use serde::{Deserialize, Serialize};

/// A value filled in when an event runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placeholder {
    /// The song that started, as `!song current` shows it.
    Song,
    /// Where the song comes from, e.g. `YouTube`.
    Source,
    /// The value of the same field in `[teamspeak]` (`nickname` or `description`).
    Default,
}

impl Placeholder {
    const ALL: [Placeholder; 3] = [Placeholder::Song, Placeholder::Source, Placeholder::Default];

    /// The name between the braces.
    pub fn name(self) -> &'static str {
        match self {
            Placeholder::Song => "song",
            Placeholder::Source => "source",
            Placeholder::Default => "default",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(String),
    Value(Placeholder),
}

/// A text with placeholders. Written back to the config exactly as it was given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Template {
    text: String,
    parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown placeholder {{{0}}}, the known ones are {{song}}, {{source}} and {{default}}")]
pub struct UnknownPlaceholder(String);

impl Template {
    /// The text as written in the config.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Every placeholder in the text, in order, repeated ones too.
    pub fn placeholders(&self) -> impl Iterator<Item = Placeholder> + '_ {
        self.parts.iter().filter_map(|part| match part {
            Part::Value(placeholder) => Some(*placeholder),
            Part::Text(_) => None,
        })
    }

    /// The text with each placeholder replaced by `value`.
    pub fn render<'a>(&self, value: impl Fn(Placeholder) -> Cow<'a, str>) -> String {
        let mut text = String::new();
        for part in &self.parts {
            match part {
                Part::Text(literal) => text.push_str(literal),
                Part::Value(placeholder) => text.push_str(&value(*placeholder)),
            }
        }
        text
    }
}

impl FromStr for Template {
    type Err = UnknownPlaceholder;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            literal.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            let len = after
                .bytes()
                .take_while(|byte| byte.is_ascii_lowercase() || *byte == b'_')
                .count();
            if len == 0 || !after[len..].starts_with('}') {
                literal.push('{');
                rest = after;
                continue;
            }
            let name = &after[..len];
            let placeholder = Placeholder::ALL
                .into_iter()
                .find(|placeholder| placeholder.name() == name)
                .ok_or_else(|| UnknownPlaceholder(name.to_owned()))?;
            if !literal.is_empty() {
                parts.push(Part::Text(mem::take(&mut literal)));
            }
            parts.push(Part::Value(placeholder));
            rest = &after[len + 1..];
        }
        literal.push_str(rest);
        if !literal.is_empty() {
            parts.push(Part::Text(literal));
        }
        Ok(Self {
            text: text.to_owned(),
            parts,
        })
    }
}

impl TryFrom<String> for Template {
    type Error = UnknownPlaceholder;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Template> for String {
    fn from(template: Template) -> Self {
        template.text
    }
}
