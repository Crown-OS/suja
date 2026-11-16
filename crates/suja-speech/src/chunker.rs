#[derive(Clone, Copy, Debug)]
pub struct ChunkerOptions {
    pub first_chunk_min_words: usize,
    pub later_chunk_min_words: usize,
}

impl Default for ChunkerOptions {
    fn default() -> Self {
        Self {
            first_chunk_min_words: 2,
            later_chunk_min_words: 4,
        }
    }
}

#[derive(Default)]
pub struct SentenceChunker {
    pending: String,
    emitted_any: bool,
    options: ChunkerOptions,
}

impl SentenceChunker {
    pub fn new(options: ChunkerOptions) -> Self {
        Self {
            options,
            ..Default::default()
        }
    }

    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.pending.push_str(delta);
        let mut chunks = Vec::new();
        while let Some(cut) = self.next_cut() {
            let chunk: String = self.pending.drain(..cut).collect();
            let chunk = chunk.trim().to_owned();
            if !chunk.is_empty() {
                chunks.push(chunk);
                self.emitted_any = true;
            }
        }
        chunks
    }

    pub fn flush(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.pending).trim().to_owned();
        self.emitted_any = false;
        (!rest.is_empty()).then_some(rest)
    }

    fn next_cut(&self) -> Option<usize> {
        let min_words = if self.emitted_any {
            self.options.later_chunk_min_words
        } else {
            self.options.first_chunk_min_words
        };
        let mut words = 0usize;
        let mut in_word = false;
        let mut chars = self.pending.char_indices().peekable();
        while let Some((index, ch)) = chars.next() {
            if ch.is_whitespace() {
                in_word = false;
            } else if !in_word {
                in_word = true;
                words += 1;
            }
            let followed_by_space = chars.peek().is_none_or(|(_, next)| next.is_whitespace());
            let sentence_end =
                matches!(ch, '.' | '!' | '?') && followed_by_space && chars.peek().is_some();
            let clause_end =
                !self.emitted_any && matches!(ch, ',' | ';' | ':') && followed_by_space;
            if (sentence_end || clause_end) && words >= min_words {
                return Some(index + ch.len_utf8());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_chunk_cuts_at_a_clause() {
        let mut chunker = SentenceChunker::default();
        let chunks = chunker.push("Sure thing, let me check the disk for you. It has");
        assert_eq!(
            chunks,
            vec!["Sure thing,", "let me check the disk for you."]
        );
        assert_eq!(chunker.flush(), Some("It has".to_owned()));
    }

    #[test]
    fn decimal_points_do_not_split() {
        let mut chunker = SentenceChunker::default();
        let chunks = chunker.push("You have 3.5 gigabytes free. Enjoy!");
        assert_eq!(chunks, vec!["You have 3.5 gigabytes free."]);
    }

    #[test]
    fn short_fragments_wait_for_more_words() {
        let mut chunker = SentenceChunker::default();
        assert!(chunker.push("Yes,").is_empty());
        assert_eq!(
            chunker.push(" that is right. And"),
            vec!["Yes, that is right."]
        );
    }
}
