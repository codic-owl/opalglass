use super::*;
#[test]
fn single_pass_tags_match_reading_and_exclude_links_comments_and_code() {
    let source = "---\ntags: [yaml]\n---\n**#bold** #plain `#code` [#label](Target.md) ![#alt](Image.png)\n\n| First | #cell |\n| --- | --- |\n| Text | #next |\n\n    #indented\n\n%%#hidden%%";
    let analysis = analyze(source);
    assert_eq!(analysis.tags, ["bold", "cell", "next", "plain", "yaml"]);
    assert_eq!(analysis.tags, crate::prepared::Prepared::new(source).tags);
    assert_eq!(analysis.references.len(), 2);
}
#[test]
fn empty_and_incomplete_wiki_links_never_create_invalid_ranges() {
    for source in [
        "[[ ]]",
        "[[\t|alias]]",
        "[[　]]",
        "[[|alias]]",
        "![[ ]]",
        "[[",
        "[[Target",
        "%%hidden%%[[ ]] [[Real]]",
    ] {
        for reference in references(source) {
            assert!(!reference.target.is_empty());
            assert!(source.get(reference.destination).is_some());
        }
    }
}
#[test]
fn references_preserve_exact_destination_ranges_and_ignore_literal_text() {
    let source = "---\nlink: '[[YAML]]'\n---\n[[Target#Heading|Alias]] ![[Picture.png|120]]\n[**label**](../A%20note.md#Part \"title\")\n![alt](<images/a (b).png>)\n[ref][r]\n\n[r]: <../Folder/File.md> 'a title'\n\n`[[code]]` `%%` [[Visible]]\n%% [[hidden]] [hidden](Hide.md) %%\n```\n[[fenced]]\n```\n";
    let refs = references(source);
    assert_eq!(
        refs.iter()
            .map(|r| &source[r.destination.clone()])
            .collect::<Vec<_>>(),
        [
            "Target#Heading",
            "Picture.png",
            "../A%20note.md#Part",
            "images/a (b).png",
            "../Folder/File.md",
            "Visible"
        ]
    );
}
#[test]
fn relative_urls_handle_unicode_reserved_characters_and_parent_segments() {
    let files = Resolver::new(["A/One.md", "Images/é (x)#1.png", "A/Target.md"].map(PathBuf::from));
    let from = Path::new("A/One.md");
    let target = Path::new("Images/é (x)#1.png");
    let url = relative_url(from, target);
    assert_eq!(url, "../Images/%C3%A9%20%28x%29%231.png");
    assert_eq!(files.resolve(from, &url, false).as_deref(), Some(target));
    assert_eq!(
        files.resolve(from, "Target#Part", true),
        Some("A/Target.md".into())
    );
    assert_eq!(files.resolve(from, "../../../outside.png", false), None);
    assert_eq!(
        files.resolve(from, "https://example.com/file.md", false),
        None
    );
    assert_eq!(
        retarget(from, Path::new("Else/Target.md"), "Target#Part", true).unwrap(),
        "/Else/Target#Part"
    );
}

#[test]
fn metadata_comment_markers_cannot_consume_the_markdown_body() {
    let source = "---\ndescription: |\n  <!--\n  %% [[Metadata]]\n---\nVisible [[Target]]\n\n%%secret [[Hidden]]%%\n\n<!-- actual comment [[Also hidden]] -->\n\n`<!-- inline %%literal%% -->`\n\n```\n<!-- fenced %%literal%% -->\n```\n";
    let header = crate::properties::frontmatter(source).unwrap().unwrap();
    let analysis = analyze(source);
    assert_eq!(analysis.hidden[0], header.range);
    assert_eq!(
        analysis
            .wiki_links
            .iter()
            .map(|l| l.target.as_str())
            .collect::<Vec<_>>(),
        ["Target"]
    );
    let prepared = crate::prepared::Prepared::new(source);
    let text: String = prepared.rows.iter().map(|r| r.text.as_ref()).collect();
    assert!(text.contains("Visible Target"));
    assert!(text.contains("<!-- inline %%literal%% -->"));
    assert!(text.contains("<!-- fenced %%literal%% -->"));
    assert!(!text.contains("Metadata"));
    assert!(!text.contains("secret"));
    assert!(!text.contains("actual comment"));
    assert!(
        prepared
            .live
            .hidden
            .iter()
            .all(|r| r.start >= header.range.end)
    );
}
