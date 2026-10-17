//! Bare links decorate parsed text without changing its source mapping.
use super::{Output, Range};
use crate::markdown::{InlineStyle, Span};
use linkify::{LinkFinder, LinkKind};

fn detect(text: &str, eligible: &[Range<usize>]) -> Vec<(Range<usize>, String)> {
    let mut finder = LinkFinder::new();
    finder.url_must_have_scheme(false);
    let mut links = Vec::new();
    for range in eligible {
        for link in finder.links(&text[range.clone()]) {
            let value = link.as_str();
            let target = match link.kind() {
                LinkKind::Email => format!("mailto:{value}"),
                LinkKind::Url => {
                    let scheme_len = if value
                        .get(..8)
                        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
                    {
                        8
                    } else if value
                        .get(..7)
                        .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
                    {
                        7
                    } else if value
                        .get(..4)
                        .is_some_and(|s| s.eq_ignore_ascii_case("www."))
                    {
                        0
                    } else {
                        continue;
                    };
                    // Match Obsidian's bare-link policy: HTTP(S), www, and ASCII
                    // hosts. Explicit Markdown links may still use other hosts.
                    let host = value[scheme_len..]
                        .split(['/', '?', '#'])
                        .next()
                        .unwrap_or("");
                    if !host.is_ascii() {
                        continue;
                    }
                    if scheme_len == 0 {
                        format!("http://{value}")
                    } else {
                        value.to_owned()
                    }
                }
                _ => continue,
            };
            links.push((range.start + link.start()..range.start + link.end(), target));
        }
    }
    links
}

pub(super) fn apply(output: &mut Output, eligible: &[Range<usize>]) {
    let links = detect(&output.text, eligible);
    if links.is_empty() {
        return;
    }
    // Split at existing formatting and link boundaries, retaining bold/italic
    // styling within links. No text or byte mapping is rewritten here.
    let mut cuts = vec![0, output.text.len()];
    for span in &output.spans {
        cuts.extend([span.range.start, span.range.end]);
    }
    for (range, _) in &links {
        cuts.extend([range.start, range.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut spans: Vec<Span> = Vec::new();
    let mut original = 0;
    let mut link = 0;
    for pair in cuts.windows(2) {
        let range = pair[0]..pair[1];
        while original < output.spans.len() && output.spans[original].range.end <= range.start {
            original += 1;
        }
        while link < links.len() && links[link].0.end <= range.start {
            link += 1;
        }
        let mut style = output
            .spans
            .get(original)
            .filter(|s| s.range.contains(&range.start))
            .map(|s| s.style.clone())
            .unwrap_or_default();
        if let Some((_, target)) = links.get(link).filter(|(r, _)| r.contains(&range.start)) {
            style.target = Some(target.clone());
        }
        if style == InlineStyle::default() {
            continue;
        }
        if let Some(previous) = spans.last_mut()
            && previous.range.end == range.start
            && previous.style == style
        {
            previous.range.end = range.end;
        } else {
            spans.push(Span { range, style });
        }
    }
    output.spans = spans;
}
