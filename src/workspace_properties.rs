// Included in workspace.rs so properties use the same editor/undo/save workflow.
use opalglass::properties::yaml as property_yaml;
struct PropertyInput {
    key: Entity<Editor>,
    value: Entity<Editor>,
}
struct PropertiesPanel {
    editor: Entity<Editor>,
    revision: u64,
    original: Arc<String>,
    source: Arc<String>,
    rows: Vec<PropertyInput>,
    yaml: Entity<Editor>,
    raw: bool,
    error: Option<String>,
}
impl PropertiesPanel {
    fn rows(map: &property_yaml::Mapping, cx: &mut Context<Workspace>) -> Vec<PropertyInput> {
        map.iter()
            .map(|(key, value)| PropertyInput {
                key: cx.new(|cx| {
                    Editor::new(key.as_str().unwrap_or_default(), true, "Property name", cx)
                }),
                value: cx.new(|cx| {
                    let text = opalglass::properties::value_text(value);
                    let mut editor = Editor::new(&text, !text.contains('\n'), "Value", cx);
                    editor.set_preferences(14., false, cx);
                    editor
                }),
            })
            .collect()
    }
    fn draft(&self, cx: &App) -> Result<String, String> {
        use opalglass::properties as p;
        if self.raw {
            let (range, replacement) =
                p::replace_yaml(&self.source, self.yaml.read(cx).buffer.text())?;
            let mut output = self.source.as_ref().clone();
            output.replace_range(range, &replacement);
            return Ok(output);
        }
        let mut wanted = property_yaml::Mapping::new();
        for row in &self.rows {
            let key = row.key.read(cx).buffer.text().trim();
            if key.is_empty() {
                return Err("Enter a name for every property, or remove the empty row.".into());
            }
            let value =
                property_yaml::from_str::<property_yaml::Value>(row.value.read(cx).buffer.text())
                    .map_err(|error| format!("{key}: {error}"))?;
            if wanted
                .insert(property_yaml::Value::String(key.to_owned()), value)
                .is_some()
            {
                return Err(format!("The property {key:?} appears more than once."));
            }
        }
        let original = p::read(&self.source)?;
        let mut output = self.source.as_ref().clone();
        for key in original.keys().filter(|key| !wanted.contains_key(*key)) {
            output = p::set(&output, key.as_str().unwrap(), None)?;
        }
        for (key, value) in wanted {
            output = p::set(&output, key.as_str().unwrap(), Some(value))?;
        }
        Ok(output)
    }
}
impl Workspace {
    fn open_properties(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let source = tab.editor.read(cx).buffer.snapshot();
        let header = match opalglass::properties::frontmatter(&source) {
            Ok(header) => header,
            Err(error) => {
                self.message(
                    format!("{error} Fix the delimiter in Source view first."),
                    true,
                    cx,
                );
                return;
            }
        };
        let yaml = header.map_or("", |h| &source[h.yaml]);
        let parsed = opalglass::properties::parse(yaml);
        let rows = PropertiesPanel::rows(&parsed.as_ref().cloned().unwrap_or_default(), cx);
        let yaml = cx.new(|cx| Editor::new(yaml, false, "tags: [ideas, work]\nstatus: draft", cx));
        self.properties_panel = Some(PropertiesPanel {
            editor: tab.editor.clone(),
            revision: tab.editor.read(cx).buffer.revision(),
            original: source.clone(),
            source,
            rows,
            yaml,
            raw: parsed.is_err(),
            error: parsed.err(),
        });
        self.close_completion(cx);
        self.focus.focus(window);
        cx.notify();
    }
    fn apply_properties(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.properties_panel else {
            return;
        };
        if panel.editor.read(cx).buffer.revision() != panel.revision
            || !self.tabs.iter().any(|tab| tab.editor == panel.editor)
        {
            self.properties_panel.as_mut().unwrap().error = Some("The note changed while properties were open. Close and reopen properties to edit the current version.".into());
            cx.notify();
            return;
        }
        match panel.draft(cx) {
            Ok(draft) => {
                let old_range = opalglass::properties::frontmatter(&panel.original)
                    .ok()
                    .flatten()
                    .map_or_else(
                        || {
                            let start = usize::from(panel.original.starts_with('\u{feff}')) * 3;
                            start..start
                        },
                        |h| h.range,
                    );
                let replacement = opalglass::properties::frontmatter(&draft)
                    .ok()
                    .flatten()
                    .map_or("", |h| &draft[h.range]);
                if &panel.original[old_range.clone()] != replacement {
                    panel.editor.update(cx, |editor, cx| {
                        editor.buffer.replace(old_range, replacement);
                        editor.reveal_cursor(cx);
                    });
                }
                self.properties_panel = None;
                self.focus_document(window, cx);
                self.message("Properties updated", false, cx);
            }
            Err(error) => {
                self.properties_panel.as_mut().unwrap().error = Some(error);
                cx.notify();
            }
        }
    }
    fn switch_properties_mode(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = &self.properties_panel else {
            return;
        };
        let raw = panel.raw;
        let draft = match panel.draft(cx) {
            Ok(draft) => draft,
            Err(error) => {
                self.properties_panel.as_mut().unwrap().error = Some(error);
                cx.notify();
                return;
            }
        };
        let yaml = opalglass::properties::frontmatter(&draft)
            .ok()
            .flatten()
            .map_or("", |h| &draft[h.yaml]);
        let rows =
            PropertiesPanel::rows(&opalglass::properties::parse(yaml).unwrap_or_default(), cx);
        let panel = self.properties_panel.as_mut().unwrap();
        panel.rows = rows;
        panel
            .yaml
            .update(cx, |editor, cx| editor.set_text(yaml, cx));
        // Preserve this draft as the basis for subsequent per-property patches.
        panel.source = Arc::new(draft);
        panel.raw = !raw;
        panel.error = None;
        cx.notify();
    }
    fn render_properties(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.properties_panel.as_ref().unwrap();
        let mut fields = div()
            .id("property-fields")
            .max_h(px(380.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2();
        if panel.raw {
            fields = fields.child(
                div()
                    .h(px(350.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .rounded_md()
                    .child(panel.yaml.clone()),
            );
        } else {
            for (index, row) in panel.rows.iter().enumerate() {
                let lines = row.value.read(cx).buffer.text().lines().count().clamp(1, 4);
                let height = if lines == 1 {
                    42.
                } else {
                    lines as f32 * 24. + 36.
                };
                fields = fields.child(
                    div()
                        .flex()
                        .gap_2()
                        .items_start()
                        .child(
                            div()
                                .w(px(150.))
                                .h(px(42.))
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(LINE))
                                .debug_selector(move || format!("property-key-{index}"))
                                .child(row.key.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(height))
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(LINE))
                                .debug_selector(move || format!("property-value-{index}"))
                                .child(row.value.clone()),
                        )
                        .child(
                            button(("remove-property", index), "×")
                                .debug_selector(move || format!("remove-property-{index}"))
                                .tooltip(|_, cx| {
                                    cx.new(|_| PathTooltip("Remove property".into())).into()
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(panel) = &mut this.properties_panel {
                                        panel.rows.remove(index);
                                    }
                                    cx.notify();
                                })),
                        ),
                );
            }
            if panel.rows.is_empty() {
                fields = fields.child(caption(
                    "No properties yet. Add tags, a status, a date, or your own fields.",
                ));
            }
        }
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x05070bb0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(680.))
                    .max_w(relative(0.95))
                    .p_5()
                    .rounded_lg()
                    .bg(rgb(PANEL))
                    .border_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(div().text_lg().child("Properties"))
                            .child(
                                button(
                                    "properties-mode",
                                    if panel.raw {
                                        "Property fields"
                                    } else {
                                        "Edit YAML"
                                    },
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.switch_properties_mode(cx)),
                                ),
                            ),
                    )
                    .child(caption(
                        "Values accept text, numbers, true/false, and lists such as [ideas, work].",
                    ))
                    .child(fields)
                    .when(!panel.raw, |el| {
                        el.child(
                            button("add-property", "+ Add property")
                                .debug_selector(|| "add-property".into())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let row = PropertyInput {
                                        key: cx
                                            .new(|cx| Editor::new("", true, "Property name", cx)),
                                        value: cx.new(|cx| Editor::new("", true, "Value", cx)),
                                    };
                                    row.key.read(cx).focus.focus(window);
                                    if let Some(panel) = &mut this.properties_panel {
                                        panel.rows.push(row);
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .children(panel.error.as_ref().map(|error| {
                        div()
                            .text_sm()
                            .text_color(rgb(0xf1b18e))
                            .child(error.clone())
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("cancel-properties", "Cancel").on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.properties_panel = None;
                                    this.focus_document(window, cx);
                                    cx.notify();
                                },
                            )))
                            .child(
                                button("apply-properties", "Apply")
                                    .debug_selector(|| "apply-properties".into())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_properties(window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
