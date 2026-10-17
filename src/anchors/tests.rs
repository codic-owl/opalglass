use super::*;

#[test]
fn headings_follow_parser_formatting_setext_unicode_and_sections() {
    let source = "---\ntitle: '# Hidden'\n---\n%%# Hidden%%\n# **Éclair** &amp; `Tea`\n\nBody\n\nChild\n-----\n\nNested\n\n## Other\n\nChild\n-----\n\n# End\n\n```\n# Fake\n```\n";
    let index = Index::parse(source);
    assert_eq!(index.headings.len(), 5);
    let top = index.target("%C3%89clair%20%26%20Tea").unwrap();
    assert_eq!(
        top,
        source.find("# **").unwrap()..source.find("# End").unwrap()
    );
    assert_eq!(index.target("éclair--tea"), Some(top));
    let child = index.target("Éclair & Tea#Child").unwrap();
    assert_eq!(
        child,
        source.find("Child").unwrap()..source.find("## Other").unwrap()
    );
    assert!(
        index.target("Other#Child").is_none(),
        "a same-level heading is not a child"
    );
    assert!(index.target("Fake").is_none());
    assert!(index.target("Hidden").is_none());
    assert!(index.target("Missing").is_none());
}

#[test]
fn block_ids_cover_paragraphs_list_items_and_standalone_structures() {
    let source = "A multiline\nparagraph 👩‍💻 ^para\n\n- First ^item\n- Second\n\n^list\n\n> [!note]- Folded\n> Content\n\n^quote\n\n| A | B |\n| - | - |\n| c | d |\n\n^table\n\n```rs\nlet x = 1;\n```\n\n^code\n";
    let index = Index::parse(source);
    for (id, start) in [
        ("para", "A multiline"),
        ("item", "- First"),
        ("list", "- First"),
        ("quote", "> [!note]"),
        ("table", "| A |"),
        ("code", "```rs"),
    ] {
        assert_eq!(
            index.target(&format!("^{id}")).map(|r| r.start),
            source.find(start),
            "{id}"
        );
    }
    let block = &index.blocks["item"];
    assert!(source[block.source.clone()].contains("First"));
    assert!(!source[block.source.clone()].contains("Second"));
    assert_eq!(&source[block.marker.clone()], "^item");
}

#[test]
fn ids_ignore_hidden_code_invalid_and_nonterminal_markers() {
    let source = "---\nid: ^yaml\n---\n%%Hidden ^comment%%\n\n`^inline`\n\n```\ntext ^fenced\n```\n\nText \\^escaped\n\nText ^bad_id\n\nText ^é\n\nText ^middle more\n\n> text ^quote\n\n| cell ^table |\n| --- |\n\nFirst ^same\n\nSecond ^same\n";
    let index = Index::parse(source);
    assert_eq!(index.blocks.len(), 1, "{:?}", index.blocks);
    assert_eq!(index.markers.len(), 2);
    assert!(index.markers.iter().all(|r| &source[r.clone()] == "^same"));
    assert_eq!(
        index.target("^same").unwrap().start,
        source.find("First").unwrap()
    );
    assert!(index.target("^SAME").is_none());
}
