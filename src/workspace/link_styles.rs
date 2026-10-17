// Shared link resolution and display styles; no filesystem reads on UI frames.
use super::*;

impl Workspace {
    pub(super) fn update_link_styles(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let revision = (tab.prepared_revision, self.revision);
        // Resolve only when the document analysis or vault index changes. Never
        // apply offsets from an older analysis to a buffer with intervening edits.
        if tab.link_style_revision == revision
            || tab.prepared_revision != tab.editor.read(cx).buffer.revision()
        {
            return;
        }
        let links = source_ranges(tab.prepared.links.iter().map(|link| link.range.clone()));
        let missing = source_ranges(
            tab.prepared
                .links
                .iter()
                .filter(|link| self.target_missing(&tab.path, &link.target))
                .map(|link| link.range.clone()),
        );
        let mut code_blocks: Vec<std::ops::Range<usize>> =
            tab.prepared.frontmatter.iter().cloned().collect();
        for row in &tab.prepared.rows {
            if matches!(row.kind, Kind::Code) {
                if let Some(last) = code_blocks.last_mut()
                    && last.end == row.source.start
                {
                    last.end = row.source.end;
                } else {
                    code_blocks.push(row.source.clone());
                }
            }
        }
        let tab = &mut self.tabs[self.active];
        tab.link_style_revision = revision;
        let syntax = tab.prepared.source_syntax.clone();
        let live = tab.prepared.live.clone();
        let prepared_revision = tab.prepared_revision;
        tab.editor.update(cx, |editor, cx| {
            editor.set_code_blocks(code_blocks);
            editor.source_syntax = syntax;
            editor.set_live_context(live, prepared_revision);
            editor.set_source_links(links, missing);
            cx.notify();
        });
    }
    // Existing host-only interaction fixtures use this convenience wrapper.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(super) fn link_highlight(&self, target: &str) -> HighlightStyle {
        let origin = self
            .tabs
            .get(self.active)
            .map_or_else(PathBuf::new, |tab| tab.path.clone());
        self.link_highlight_from(&origin, target)
    }

    pub(super) fn link_highlight_from(
        &self,
        origin: &std::path::Path,
        target: &str,
    ) -> HighlightStyle {
        let missing = self.target_missing(origin, target);
        HighlightStyle {
            color: Some(rgb(if missing { MISSING_LINK } else { ACCENT }).into()),
            underline: Some(if missing {
                missing_link_underline()
            } else {
                UnderlineStyle {
                    thickness: px(1.),
                    color: Some(rgb(0x786caa).into()),
                    wavy: false,
                }
            }),
            ..Default::default()
        }
    }
    fn target_missing(&self, from: &std::path::Path, target: &str) -> bool {
        if external_link(target)
            || target.starts_with(markdown::TAG_TARGET_PREFIX)
            || target.starts_with(opalglass::footnotes::TARGET_PREFIX)
            || target.starts_with(opalglass::footnotes::BACK_PREFIX)
        {
            return false;
        }
        let Some(vault) = &self.vault else {
            return false;
        };
        let Some(path) = vault
            .resolve(from, target)
            .or_else(|| vault.resolve_url(from, target))
        else {
            return vault.resolve_attachment(from, target).is_none();
        };
        if !vault.contains_path(&path) {
            return false;
        }
        let Some((_, fragment)) = target.split_once('#') else {
            return false;
        };
        let anchors = if let Some(tab) = self.tabs.iter().find(|t| t.path == path) {
            tab.prepared.complete.then_some(&tab.prepared.anchors)
        } else {
            vault.anchors(&path)
        };
        // Discovery/opening prefixes cannot prove that a later target is missing.
        anchors.is_some_and(|index| index.target(fragment).is_none())
    }
}

// Nested image/link syntax can overlap. The editor's binary searches require
// both starts and ends to increase, so merge intersecting styling ranges.
fn source_ranges(
    ranges: impl Iterator<Item = std::ops::Range<usize>>,
) -> Vec<std::ops::Range<usize>> {
    let mut result: Vec<std::ops::Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(last) = result.last_mut()
            && range.start < last.end
        {
            last.end = last.end.max(range.end);
        } else {
            result.push(range);
        }
    }
    result
}
