//! Bounded diagnostic storage for reproducing timing bugs without per-call I/O.
use std::collections::VecDeque;

pub(crate) struct LogBuffer {
    limit: usize,
    bytes: usize,
    dropped: usize,
    lines: VecDeque<String>,
}

impl LogBuffer {
    pub(crate) fn new(limit: usize) -> Self {
        Self { limit, bytes: 0, dropped: 0, lines: VecDeque::new() }
    }

    pub(crate) fn push(&mut self, line: String) {
        if line.len() > self.limit {
            self.dropped += 1;
            return;
        }
        while self.bytes > self.limit - line.len() {
            if let Some(old) = self.lines.pop_front() {
                self.bytes -= old.len();
                self.dropped += 1;
            }
        }
        self.bytes += line.len();
        self.lines.push_back(line);
    }

    pub(crate) fn drain(&mut self) -> String {
        let mut output = format!("Buffered trace: dropped {} earlier/oversized records\n", self.dropped);
        for line in self.lines.drain(..) { output.push_str(&line); }
        self.bytes = 0;
        self.dropped = 0;
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_retains_newest_whole_records_and_reports_loss() {
        let mut buffer = LogBuffer::new(8);
        for line in ["one\n", "two\n", "三\n"] { buffer.push(line.into()); }
        assert_eq!(buffer.bytes, 8);
        assert_eq!(buffer.drain(), "Buffered trace: dropped 1 earlier/oversized records\ntwo\n三\n");
        assert_eq!(buffer.bytes, 0);
        buffer.push("new\n".into());
        assert_eq!(buffer.drain(), "Buffered trace: dropped 0 earlier/oversized records\nnew\n");
    }

    #[test]
    fn oversized_record_does_not_evict_existing_records() {
        let mut buffer = LogBuffer::new(4);
        buffer.push("ok\n".into());
        buffer.push("too large\n".into());
        assert_eq!(buffer.drain(), "Buffered trace: dropped 1 earlier/oversized records\nok\n");
        let mut empty = LogBuffer::new(0);
        empty.push("x".into());
        assert_eq!(empty.drain(), "Buffered trace: dropped 1 earlier/oversized records\n");
    }
}
