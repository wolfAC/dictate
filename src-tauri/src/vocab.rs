//! The personal dictionary and text replacements.
//!
//! Dictionary words are passed to the speech model as a hint so it spells them the
//! user's way, and any differently-cased spelling that still slips through is fixed
//! afterwards. Replacements turn a spoken phrase ("my email") into fixed text.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    /// What the user says.
    pub from: String,
    /// What gets typed instead.
    pub to: String,
}

/// Whisper only looks at roughly the last 220 tokens of its prompt.
const MAX_HINT_CHARS: usize = 800;
const MAX_DICTIONARY_CHARS: usize = 450;

/// Builds the speech model prompt. Dictionary words go last, where Whisper weighs them most.
pub fn hint(dictionary: &[String], command_hint: Option<&str>) -> Option<String> {
    let mut glossary = String::new();
    for word in dictionary.iter().map(|w| w.trim()).filter(|w| !w.is_empty()) {
        if glossary.len() + word.len() > MAX_DICTIONARY_CHARS {
            break;
        }
        glossary.push_str(if glossary.is_empty() { "Glossary: " } else { ", " });
        glossary.push_str(word);
    }
    if !glossary.is_empty() {
        glossary.push('.');
    }

    let mut out = String::new();
    if let Some(commands) = command_hint {
        let room = MAX_HINT_CHARS.saturating_sub(glossary.len() + 1);
        out.push_str(truncate(commands, room));
    }
    if !glossary.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&glossary);
    }
    (!out.is_empty()).then_some(out)
}

/// Applies replacements, then the dictionary's spellings.
pub fn apply(text: &str, dictionary: &[String], replacements: &[Replacement]) -> String {
    // The whole dictation is a trigger ("My email."): type just the replacement,
    // without the punctuation the speech model added.
    let spoken = phrase_key(text);
    if let Some(r) = replacements.iter().find(|r| !r.from.trim().is_empty() && phrase_key(&r.from) == spoken) {
        return r.to.clone();
    }

    let mut out = text.to_string();
    for r in replacements.iter().filter(|r| !r.from.trim().is_empty()) {
        out = replace_phrase(&out, r.from.trim(), &r.to);
    }
    for word in dictionary.iter().map(|w| w.trim()).filter(|w| !w.is_empty()) {
        out = replace_phrase(&out, word, word);
    }
    out
}

/// Lowercase words only, so "My email." matches "my email".
fn phrase_key(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Replaces whole-word, case-insensitive occurrences of `phrase` with `with`.
fn replace_phrase(text: &str, phrase: &str, with: &str) -> String {
    let hay: Vec<char> = text.chars().collect();
    let needle: Vec<char> = phrase.chars().collect();
    if needle.is_empty() || needle.len() > hay.len() {
        return text.to_string();
    }
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let is_word = |c: char| c.is_alphanumeric() || c == '\'';

    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < hay.len() {
        let fits = i + needle.len() <= hay.len()
            && (i == 0 || !is_word(hay[i - 1]))
            && hay.get(i + needle.len()).is_none_or(|&c| !is_word(c))
            && needle.iter().zip(&hay[i..]).all(|(&n, &h)| same(n, h));
        if fits {
            out.push_str(with);
            i += needle.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rep(from: &str, to: &str) -> Replacement {
        Replacement { from: from.into(), to: to.into() }
    }

    #[test]
    fn whole_dictation_trigger_types_only_the_replacement() {
        let r = [rep("my email", "anbu@example.com")];
        assert_eq!(apply("My email.", &[], &r), "anbu@example.com");
        assert_eq!(apply("  my EMAIL! ", &[], &r), "anbu@example.com");
    }

    #[test]
    fn replaces_inside_sentences_on_word_boundaries() {
        let r = [rep("my email", "anbu@example.com"), rep("brb", "be right back")];
        assert_eq!(
            apply("Send it to my email please.", &[], &r),
            "Send it to anbu@example.com please."
        );
        // No partial-word matches.
        assert_eq!(apply("The brbx stays.", &[], &r), "The brbx stays.");
        assert_eq!(apply("BRB, coffee.", &[], &r), "be right back, coffee.");
    }

    #[test]
    fn dictionary_fixes_spelling_case() {
        let dict = ["GitHub".to_string(), "Tauri".to_string(), "iOS".to_string()];
        assert_eq!(
            apply("push it to github and test on ios with tauri.", &dict, &[]),
            "push it to GitHub and test on iOS with Tauri."
        );
        assert_eq!(apply("Githubbing is not a word.", &dict, &[]), "Githubbing is not a word.");
    }

    #[test]
    fn hint_puts_dictionary_last_and_stays_short() {
        let dict = ["Anbu".to_string(), "Byepo".to_string()];
        let long = "x".repeat(2000);
        let h = hint(&dict, Some(&long)).unwrap();
        assert!(h.ends_with("Glossary: Anbu, Byepo."), "{h}");
        assert!(h.len() <= MAX_HINT_CHARS);
        assert_eq!(hint(&[], None), None);
        assert_eq!(hint(&dict, None).unwrap(), "Glossary: Anbu, Byepo.");
    }
}
