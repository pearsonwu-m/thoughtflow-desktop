//! Minimal Server-Sent Events parser for the Messages streaming API.
//!
//! Bytes are buffered until a full event (terminated by a blank line) has
//! arrived, so multi-byte UTF-8 characters split across network chunks are
//! decoded correctly.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some((end, sep_len)) = find_boundary(&self.buf) {
            let raw: Vec<u8> = self.buf.drain(..end + sep_len).collect();
            if let Some(event) = parse_event(&String::from_utf8_lossy(&raw[..end])) {
                events.push(event);
            }
        }
        events
    }
}

fn find_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let crlf = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

fn parse_event(block: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in block.lines() {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "data" => data.push(value),
            _ => {}
        }
    }
    if event.is_none() && data.is_empty() {
        return None;
    }
    Some(SseEvent {
        event,
        data: data.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_split_across_chunks() {
        let mut parser = SseParser::default();
        assert!(parser.feed(b"event: ping\nda").is_empty());
        let events = parser.feed(b"ta: {\"type\":\"ping\"}\n\nevent: message_stop\ndata: {}\n\n");
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event: Some("ping".into()),
                    data: "{\"type\":\"ping\"}".into()
                },
                SseEvent {
                    event: Some("message_stop".into()),
                    data: "{}".into()
                },
            ]
        );
    }

    #[test]
    fn handles_crlf_comments_and_multibyte_splits() {
        let mut parser = SseParser::default();
        let text = "data: {\"t\":\"思考\"}\r\n\r\n".as_bytes();
        // Split in the middle of a multi-byte character.
        let (a, b) = text.split_at(13);
        assert!(parser.feed(a).is_empty());
        let events = parser.feed(b);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"t\":\"思考\"}");

        let mut parser = SseParser::default();
        assert!(parser.feed(b": keep-alive\n\n").is_empty());
    }
}
