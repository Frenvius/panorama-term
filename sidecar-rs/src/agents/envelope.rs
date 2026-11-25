use crate::agents::mail::Message;

pub const INLINE_MAX_BYTES: usize = 2048;
pub const INLINE_HEAD_BYTES: usize = 600;
pub const BATCH_MAX_BYTES: usize = 8192;
pub const BATCH_MAX_MESSAGES: usize = 5;
const FLAT_JOIN: &str = " / ";

pub fn header(m: &Message) -> String {
    if m.notice {
        return "[panorama notice]".into();
    }
    let suffix = match (&m.reply_to, m.expects_reply) {
        (Some(parent), _) => format!(", reply to {parent}"),
        (None, true) => ", reply expected".into(),
        (None, false) => String::new(),
    };
    format!("[panorama msg {} from {}{suffix}]", m.id, m.from_name)
}

fn cut_at_char(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub fn body(m: &Message, cli: &str) -> String {
    if m.text.len() <= INLINE_MAX_BYTES {
        return m.text.trim_end().to_string();
    }
    format!("{} ... (full text: {cli} agent read {})", cut_at_char(&m.text, INLINE_HEAD_BYTES).trim_end(), m.id)
}

pub fn flatten(text: &str) -> String {
    text.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(FLAT_JOIN)
}

pub fn line(m: &Message, cli: &str, bracketed: bool) -> String {
    let text = format!("{} {}", header(m), body(m, cli));
    if bracketed { text } else { flatten(&text) }
}

pub struct Batch {
    pub text: String,
    pub ids: Vec<String>,
}

pub fn batch(queue: &[&Message], cli: &str, bracketed: bool, prefix: Option<&str>) -> Option<Batch> {
    let mut lines = Vec::new();
    let mut ids = Vec::new();
    let mut size = prefix.map_or(0, str::len);
    for m in queue.iter().take(BATCH_MAX_MESSAGES) {
        let l = line(m, cli, bracketed);
        if !ids.is_empty() && size + l.len() + 1 > BATCH_MAX_BYTES {
            break;
        }
        size += l.len() + 1;
        lines.push(l);
        ids.push(m.id.clone());
    }
    if ids.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = prefix.map(|p| if bracketed { p.to_string() } else { flatten(p) }).into_iter().collect();
    parts.extend(lines);
    let text = parts.join(if bracketed { "\n" } else { FLAT_JOIN });
    Some(Batch { text, ids })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::mail::tests::{agent, msg};
    use crate::agents::mail::Party;

    fn named(id: &str, text: &str) -> Message {
        Message { from_name: "planner".into(), text: text.into(), ..msg(id, agent("a"), agent("b")) }
    }

    #[test]
    fn header_shapes_are_fixed() {
        let plain = named("m4k2p9", "hi");
        assert_eq!(header(&plain), "[panorama msg m4k2p9 from planner]");
        let ask = Message { expects_reply: true, ..plain.clone() };
        assert_eq!(header(&ask), "[panorama msg m4k2p9 from planner, reply expected]");
        let reply = Message { reply_to: Some("m1".into()), from_name: "rev".into(), ..named("m7x1aa", "ok") };
        assert_eq!(line(&reply, "/cli", true), "[panorama msg m7x1aa from rev, reply to m1] ok");
        let notice = Message { notice: true, from: Party::User, ..named("n1", "m1 to rev was dropped: agent deleted.") };
        assert_eq!(line(&notice, "/cli", true), "[panorama notice] m1 to rev was dropped: agent deleted.");
    }

    #[test]
    fn long_text_is_cut_with_a_read_pointer() {
        let long = named("m1", &"\u{e9}".repeat(INLINE_MAX_BYTES));
        let b = body(&long, "/cli");
        assert!(b.ends_with(" ... (full text: /cli agent read m1)"), "{b}");
        assert!(b.len() < INLINE_HEAD_BYTES + 60);
        let exact = named("m2", &"x".repeat(INLINE_MAX_BYTES));
        assert_eq!(body(&exact, "/cli").len(), INLINE_MAX_BYTES);
    }

    #[test]
    fn newlines_are_kept_only_with_bracketed_paste() {
        let m = named("m1", "line one\nline two\n");
        assert_eq!(line(&m, "/cli", true), "[panorama msg m1 from planner] line one\nline two");
        assert_eq!(line(&m, "/cli", false), "[panorama msg m1 from planner] line one / line two");
    }

    #[test]
    fn batches_cap_count_and_bytes_and_keep_order() {
        let msgs: Vec<Message> = (0..7).map(|i| named(&format!("m{i}"), "short")).collect();
        let refs: Vec<&Message> = msgs.iter().collect();
        let b = batch(&refs, "/cli", true, None).unwrap();
        assert_eq!(b.ids, ["m0", "m1", "m2", "m3", "m4"]);
        assert_eq!(b.text.lines().count(), 5);
        let big: Vec<Message> = (0..4).map(|i| named(&format!("b{i}"), &"y".repeat(INLINE_MAX_BYTES))).collect();
        let refs: Vec<&Message> = big.iter().collect();
        let b = batch(&refs, "/cli", true, None).unwrap();
        assert_eq!(b.ids.len(), 3, "three 2 KB lines fit in 8 KB, a fourth does not");
        let flat = batch(&refs[..2], "/cli", false, Some("block line 1\nblock line 2")).unwrap();
        assert!(!flat.text.contains('\n'));
        assert!(flat.text.starts_with("block line 1 / block line 2 / [panorama msg b0"));
        assert!(batch(&[], "/cli", true, None).is_none());
    }
}
