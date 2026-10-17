use crate::prepared::Prepared;

#[test]
fn chunked_labels_are_one_occurrence_and_keep_surrounding_text() {
    let source = format!("Before ![[Target|{}]] after", "**é** ".repeat(1500));
    let prepared = Prepared::new(&source);
    assert!(prepared.rows.len() > 2);
    assert_eq!(prepared.embeds.len(), 1);
    let embed = &prepared.embeds[0];
    assert!(embed.rows.len() > 2);
    assert_eq!(embed.request.target.as_ref(), "Target");
    let first = &prepared.rows[embed.rows.start];
    assert_eq!(&first.text[..embed.request.range.start], "Before ");
    let last = &prepared.rows[embed.rows.end - 1];
    assert_eq!(&last.text[embed.last_end..], " after");
}

#[test]
fn adjacent_embeds_and_separate_paragraphs_retain_their_own_identity() {
    let prepared = Prepared::new("![[A]]![[A]]\n\n![[A]]\n\n> [!note] ![[A]]\n");
    assert_eq!(prepared.embeds.len(), 4);
    assert!(prepared.embeds.iter().all(|embed| embed.rows.len() == 1));
    assert_ne!(
        prepared.embeds[0].request.image_start,
        prepared.embeds[1].request.image_start
    );
}
