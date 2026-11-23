//! What diagnostics keep of a name (`docs/diagnostics.md`): a file's base name when it looks like
//! a file's, never a folder's; no quoted words; and never the user's own name or profile folder.

use std::path::Path;
use std::sync::OnceLock;

/// The most characters of a file name a line keeps.
pub(crate) const MOST_NAME: usize = 64;
/// What stands in for a name that is not kept.
pub(crate) const HIDDEN: &str = "...";
/// What stands in for the user's name or profile folder.
pub(crate) const USER: &str = "<user>";
/// Shorter words are not replaced: they would match inside every other word.
const SHORTEST_IDENTITY: usize = 2;

static IDENTITY: OnceLock<Vec<String>> = OnceLock::new();

/// Remembers who the user is, once, so nothing written names them: the profile folder in full,
/// its last part and the account name, longest first.
pub(crate) fn capture_identity() {
    let _ = IDENTITY.get_or_init(|| {
        let var = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        identity_words(var("USERPROFILE"), var("USERNAME"))
    });
}

/// The words [`capture_identity`] keeps out, longest first.
pub(crate) fn identity_words(profile: Option<String>, user: Option<String>) -> Vec<String> {
    let last = profile
        .as_deref()
        .and_then(|p| p.rsplit(['\\', '/']).find(|s| !s.is_empty()))
        .map(str::to_string);
    let mut words: Vec<String> = [profile, last, user]
        .into_iter()
        .flatten()
        .filter(|w| w.chars().count() >= SHORTEST_IDENTITY)
        .collect();
    words.sort_by_key(|w| std::cmp::Reverse(w.len()));
    words.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    words
}

/// `text` with every word of `words` replaced by [`USER`], ignoring ASCII case.
pub(crate) fn scrub_words(text: &str, words: &[String]) -> String {
    let mut out = text.to_string();
    for word in words {
        out = replace_ignoring_case(&out, word, USER);
    }
    out
}

/// `text` with the user's name and profile folder replaced, once they are known.
pub(crate) fn scrub(text: &str) -> String {
    match IDENTITY.get() {
        Some(words) => scrub_words(text, words),
        None => text.to_string(),
    }
}

fn replace_ignoring_case(text: &str, word: &str, with: &str) -> String {
    let (lower, find) = (text.to_ascii_lowercase(), word.to_ascii_lowercase());
    if find.is_empty() || lower.len() != text.len() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find(&find) {
        let start = at + found;
        out.push_str(&text[at..start]);
        out.push_str(with);
        at = start + find.len();
    }
    out.push_str(&text[at..]);
    out
}

/// True for a name with an extension, as files have and folders usually do not: a dot after
/// the first character, followed by a letter or digit.
pub(crate) fn looks_like_file(name: &str) -> bool {
    name.char_indices()
        .skip(1)
        .any(|(i, c)| c == '.' && name[i + 1..].starts_with(|n: char| n.is_ascii_alphanumeric()))
}

/// A file's name without its folder, with spaces and control characters as `_`, cut short.
/// A name that does not look like a file's (a folder, say) becomes [`HIDDEN`].
pub(crate) fn base_name(path: &Path) -> String {
    let Some(name) = path.file_name() else {
        return "-".to_string();
    };
    let name = name.to_string_lossy();
    if !looks_like_file(&name) {
        return HIDDEN.to_string();
    }
    let kept: String = name
        .chars()
        .map(|c| match c.is_whitespace() || c.is_control() {
            true => '_',
            false => c,
        })
        .take(MOST_NAME)
        .collect();
    scrub(&kept)
}

/// Free text with quoted words replaced by `"..."` and each path cut to its base name, for a
/// crash note: such text may name a layer, a script's words or a person's folders.
pub(crate) fn reduce_paths(text: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let ends_run =
            matches!(c, '\n' | '\r' | '<' | '>' | '|') || (c == ':' && chars.peek() == Some(&' '));
        if c == '"' {
            out.push_str(&cut_run(&std::mem::take(&mut run)));
            out.push_str(if quoted { "\"" } else { "\"..." });
            quoted = !quoted;
        } else if quoted {
        } else if ends_run {
            out.push_str(&cut_run(&std::mem::take(&mut run)));
            out.push(c);
        } else {
            run.push(c);
        }
    }
    if quoted {
        out.push('"');
    }
    out.push_str(&cut_run(&run));
    scrub(&out)
}

/// Ends a name inside free text: what may follow a file name in a message.
fn ends_name(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | ')' | ',' | ';')
}

/// Drops everything from the start of the first path in `run` to its last separator. Folder
/// names may hold spaces, brackets and commas, so the path runs to the last separator however
/// many there are; what follows it is kept only if it looks like a file's name, and otherwise
/// the rest of the run goes too.
fn cut_run(run: &str) -> String {
    let is_sep = |c: char| c == '\\' || c == '/';
    let (Some(first), Some(last)) = (run.find(is_sep), run.rfind(is_sep)) else {
        return run.to_string();
    };
    let start = run[..first].rfind(ends_name).map_or(0, |at| at + 1);
    let tail = &run[last + 1..];
    let name_end = tail.find(ends_name).unwrap_or(tail.len());
    let (name, rest) = tail.split_at(name_end);
    // After a folder, the words up to the end of the run may be more of its name.
    match looks_like_file(name) {
        true => format!("{}{name}{rest}", &run[..start]),
        false => format!("{}{HIDDEN}", &run[..start]),
    }
}
