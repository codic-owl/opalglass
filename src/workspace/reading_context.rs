//! Immutable source identity and presentation state for one Reading instance.
use super::*;
use embedded_documents::{Document, SourceToken};

#[derive(Clone)]
pub(super) struct ReadingContext {
    pub(super) path: PathBuf,
    pub(super) prepared: Arc<Prepared>,
    pub(super) projection: opalglass::callouts::Projection,
    pub(super) instance: u64,
    pub(super) revision: Option<u64>,
    pub(super) token: Option<SourceToken>,
    pub(super) source: Option<Arc<String>>,
}

impl ReadingContext {
    pub(super) fn host(tab: &Tab) -> Self {
        Self {
            path: tab.path.clone(),
            prepared: tab.prepared.clone(),
            projection: tab.preview_projection.clone(),
            instance: 0,
            revision: Some(tab.prepared_revision),
            token: Some(SourceToken::Open {
                editor: tab.editor.entity_id(),
                revision: tab.prepared_revision,
            }),
            source: None,
        }
    }

    pub(super) fn embedded(
        document: &Document,
        projection: opalglass::callouts::Projection,
        instance: u64,
    ) -> Self {
        Self {
            path: document.path.clone(),
            prepared: document.prepared.clone(),
            projection,
            instance,
            revision: match document.token {
                SourceToken::Open { revision, .. } => Some(revision),
                _ => None,
            },
            token: Some(document.token),
            source: Some(document.source.clone()),
        }
    }

    pub(super) fn id(&self, kind: impl Into<SharedString>, index: usize) -> ElementId {
        scoped_id(self.instance, kind, index)
    }

    pub(super) fn selector(&self, base: impl AsRef<str>) -> String {
        scoped_selector(self.instance, base.as_ref())
    }

    pub(super) fn matches_tab(&self, tab: &Tab, cx: &App) -> bool {
        if tab.path != self.path {
            return false;
        }
        let editor = tab.editor.read(cx);
        match self.token {
            Some(SourceToken::Open {
                editor: identity,
                revision,
            }) => tab.editor.entity_id() == identity && editor.buffer.revision() == revision,
            Some(SourceToken::Saved { .. }) => self
                .source
                .as_ref()
                .is_some_and(|source| source.as_str() == editor.buffer.text()),
            None => self.revision == Some(editor.buffer.revision()),
        }
    }
}

pub(super) fn scoped_id(instance: u64, kind: impl Into<SharedString>, index: usize) -> ElementId {
    let kind = kind.into();
    if instance == 0 {
        (kind, index).into()
    } else {
        (
            SharedString::from(format!("embed-{instance}-{kind}")),
            index,
        )
            .into()
    }
}

pub(super) fn scoped_selector(instance: u64, base: &str) -> String {
    if instance == 0 {
        base.to_owned()
    } else {
        format!("embed-{instance}-{base}")
    }
}
