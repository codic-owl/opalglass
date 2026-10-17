use super::*;
#[test]
fn quote_and_list_anchors_are_consumed_by_rules_and_tables() {
    let source = "> ***\n>\n> Paragraph\n\n> | a | b |\n> | - | - |\n> | c | d |\n>\n> After table\n\n- ***\n\n  After rule\n";
    let blocks = parse(source, &AtomicBool::new(false)).unwrap();
    for pair in blocks.windows(2) {
        assert!(
            pair[0].source.end <= pair[1].source.start,
            "{:?} {:?}",
            pair[0].source,
            pair[1].source
        );
    }
    assert_eq!(blocks[0].source.start, 0);
    assert!(matches!(blocks[0].kind, Kind::Rule));
    assert_eq!(blocks[1].text, "Paragraph");
    assert_eq!(blocks[2].source.start, source.find("> | a").unwrap());
    assert_eq!(blocks.last().unwrap().text, "After rule");
}

#[test]
fn document_context_preserves_references_nested_lists_quotes_and_tables() {
    let source = "Title\n=====\n\n3. First **bold**\n   - Child\n     > Nested quote\n4. Second\n\n| Left | Right |\n| :--- | ---: |\n| [Alias][dest] | `x\\|y` |\n| [[Target\\|alias]] | end |\n\n[dest]: <Folder/Target.md> \"title\"\n\n    indented **literal**\n";
    let blocks = parse(source, &AtomicBool::new(false)).unwrap();
    assert!(matches!(blocks[0].kind, Kind::Heading(1)));
    assert_eq!(blocks[0].text, "Title");
    assert_eq!(blocks[1].text, "3.  First bold");
    assert_eq!(blocks[2].text, "•  Child");
    assert_eq!(blocks[2].indent, 1);
    assert_eq!(blocks[3].text, "Nested quote");
    assert_eq!(blocks[3].quotes, 1);
    assert_eq!(blocks[4].text, "4.  Second");
    let tables: Vec<_> = blocks.iter().filter_map(|b| b.table.as_ref()).collect();
    assert_eq!(tables.len(), 3);
    assert_eq!(
        tables[2].cells[0].spans[0].style.target.as_deref(),
        Some("Target")
    );
    assert!(tables[0].header);
    assert_eq!(tables[1].cells[0].text.as_ref(), "Alias");
    assert_eq!(
        tables[1].cells[0].spans[0].style.target.as_deref(),
        Some("Folder/Target.md")
    );
    assert_eq!(tables[1].cells[1].text.as_ref(), "x|y");
    assert_eq!(tables[1].cells[1].alignment, Alignment::Right);
    assert!(tables[1].cells[1].spans[0].style.code);
    assert_eq!(blocks.last().unwrap().text, "indented **literal**\n");
    assert!(matches!(blocks.last().unwrap().kind, Kind::Code));
    assert!(
        blocks
            .iter()
            .all(|b| source.get(b.source.clone()).is_some())
    );
}

#[test]
fn tasks_have_exact_edit_ranges_and_code_stays_literal() {
    let source = "- [ ] **Todo**\n  - [X] Done\n\n```rust\n- [ ] literal\n```\n\nSoft\nbreak\n";
    let blocks = parse(source, &AtomicBool::new(false)).unwrap();
    assert!(matches!(blocks[0].kind, Kind::Task(false)));
    assert_eq!(blocks[0].text, "Todo");
    assert_eq!(&source[blocks[0].task.clone().unwrap()], "[ ]");
    assert!(matches!(blocks[1].kind, Kind::Task(true)));
    assert_eq!(blocks[1].indent, 1);
    assert_eq!(&source[blocks[1].task.clone().unwrap()], "[X]");
    assert_eq!(blocks[2].text, "- [ ] literal\n");
    assert_eq!(blocks[2].language.as_deref(), Some("rust"));
    assert_eq!(blocks[3].text, "Soft\nbreak");
}
#[test]
fn custom_tasks_support_all_list_markers_and_unicode_without_eating_text() {
    let source = "- [?] Question\n  * [/] **Nested**\n+ [-] Cancelled\n\n1. [é] Unicode\n2. [ ]\n\n- [?]no-space\n- [word] Not a task\n\n`- [?] literal`";
    let blocks = parse(source, &AtomicBool::new(false)).unwrap();
    let tasks: Vec<_> = blocks.iter().filter(|b| b.task.is_some()).collect();
    assert_eq!(tasks.len(), 4);
    assert_eq!(
        tasks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
        ["Question", "Nested", "Cancelled", "Unicode"]
    );
    for (task, marker) in tasks.iter().zip(["[?]", "[/]", "[-]", "[é]"]) {
        assert_eq!(&source[task.task.clone().unwrap()], marker);
    }
    assert_eq!(tasks[1].indent, 1);
    assert!(tasks[1].spans[0].style.bold);
    assert!(
        blocks
            .iter()
            .any(|b| b.text.contains("[?]no-space") && b.task.is_none())
    );
}
#[test]
fn hidden_comments_do_not_create_code_or_shift_task_edit_positions() {
    let source = "---\ntitle: Hidden\n---\n%% comment %%Visible **text**\n\n%%\n# Hidden\n%%\n\n- [ ] Real task\n\n<!-- hidden -->\n\n`%% literal %%`";
    let blocks = parse(source, &AtomicBool::new(false)).unwrap();
    assert_eq!(blocks.len(), 3);
    assert!(matches!(blocks[0].kind, Kind::Paragraph));
    assert_eq!(blocks[0].text, "Visible text");
    assert_eq!(&source[blocks[1].task.clone().unwrap()], "[ ]");
    assert_eq!(blocks[2].text, "%% literal %%");
    assert!(blocks[2].spans[0].style.code);
}
