use std::borrow::Cow;

pub fn truncate_chars(text: &str, max_chars: usize) -> Cow<'_, str> {
    match text.char_indices().nth(max_chars) {
        Some((cut, _)) => Cow::Owned(format!(
            "{}\n[truncated {} chars]",
            &text[..cut],
            text.len() - cut
        )),
        None => Cow::Borrowed(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_borrowed() {
        assert!(matches!(truncate_chars("abc", 5), Cow::Borrowed(_)));
    }

    #[test]
    fn long_text_is_cut_with_a_marker() {
        let cut = truncate_chars("abcdefgh", 3);
        assert!(cut.starts_with("abc\n[truncated 5 chars]"));
    }
}
