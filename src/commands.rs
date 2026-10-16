//! Discoverable workspace actions and small note workflows.
use crate::vault::Vault;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    OpenVault,
    NewNote,
    NewFolder,
    Save,
    Find,
    Properties,
    Mentions,
    Compare,
    Replace,
    Search,
    Source,
    Split,
    Reading,
    LivePreview,
    Outline,
    Settings,
    Back,
    Forward,
    Bookmark,
    Bookmarks,
    DailyNote,
    Template,
    Bold,
    Italic,
    Code,
    Task,
    Trash,
    RestoreTrash,
}
impl Command {
    pub const ALL: &'static [(Self, &'static str, &'static str)] = &[
        (Self::OpenVault, "Open vault", "Ctrl O"),
        (Self::NewNote, "Create new note", "Ctrl N"),
        (Self::NewFolder, "Create new folder", "Ctrl Shift N"),
        (Self::Save, "Save current note", "Ctrl S"),
        (Self::Find, "Find in current note", "Ctrl F"),
        (Self::Properties, "Edit note properties", ""),
        (Self::Mentions, "Find unlinked mentions of current note", ""),
        (Self::Compare, "Compare current note with disk", ""),
        (
            Self::Replace,
            "Search and replace across vault",
            "Ctrl Shift H",
        ),
        (Self::Search, "Search vault", "Ctrl P"),
        (Self::Source, "View: source editor", ""),
        (Self::Split, "View: split editor and reading", ""),
        (Self::Reading, "View: reading", ""),
        (
            Self::LivePreview,
            "Toggle Live Preview editing",
            "Ctrl Shift E",
        ),
        (Self::Outline, "Toggle heading outline", "Ctrl Shift O"),
        (Self::Settings, "Open settings", "Ctrl ,"),
        (Self::Back, "Navigate back", "Alt ←"),
        (Self::Forward, "Navigate forward", "Alt →"),
        (
            Self::Bookmark,
            "Toggle bookmark for current note",
            "Ctrl Shift D",
        ),
        (Self::Bookmarks, "Open bookmarks", ""),
        (Self::DailyNote, "Open today's daily note", "Ctrl Alt D"),
        (Self::Template, "Insert template", "Ctrl Alt T"),
        (Self::Bold, "Format: bold", "Ctrl Alt B"),
        (Self::Italic, "Format: italic", "Ctrl Alt I"),
        (Self::Code, "Format: inline code", "Ctrl Alt C"),
        (Self::Task, "Toggle task checkbox", "Ctrl Enter"),
        (Self::Trash, "Move current note to trash", ""),
        (Self::RestoreTrash, "Restore last deleted item", ""),
    ];
    pub fn available(self, vault: bool, note: bool) -> bool {
        match self {
            Self::OpenVault | Self::Settings => true,
            Self::NewNote
            | Self::NewFolder
            | Self::Search
            | Self::Replace
            | Self::Bookmarks
            | Self::DailyNote
            | Self::RestoreTrash => vault,
            _ => note,
        }
    }
}

pub struct TemplateValues {
    pub date: String,
    pub time: String,
    pub title: String,
}
impl TemplateValues {
    pub fn now(title: String) -> io::Result<Self> {
        let now = time::OffsetDateTime::now_local().map_err(io::Error::other)?;
        Ok(Self {
            date: now.date().to_string(),
            time: format!("{:02}:{:02}", now.hour(), now.minute()),
            title,
        })
    }
}
/// Literal single-pass substitutions; inserted values cannot become directives.
pub fn render_template(template: &str, values: &TemplateValues) -> String {
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find("}}") else {
            break;
        };
        output.push_str(match &rest[..end + 2] {
            "{{date}}" => &values.date,
            "{{time}}" => &values.time,
            "{{title}}" => &values.title,
            token => token,
        });
        rest = &rest[end + 2..];
    }
    output.push_str(rest);
    output
}
/// Open/create a daily note without modifying one that already exists.
pub fn daily_note(vault: &mut Vault, values: &TemplateValues) -> io::Result<PathBuf> {
    let path = Vault::note_path(&format!("Daily/{}", values.date))?;
    if vault.read(&path).is_ok() {
        return Ok(path);
    }
    let template = match vault.read(Path::new("Templates/Daily.md")) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => "# {{date}}\n\n".into(),
        Err(error) => return Err(error),
    };
    match vault.create(&path.to_string_lossy(), &render_template(&template, values)) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            vault.read(&path)?;
            Ok(path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daily_note_is_idempotent_and_templates_preserve_literal_values() {
        let directory = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(directory.path()).unwrap();
        vault
            .create("Templates/Daily", "# {{date}}\n{{time}} · {{title}}\n")
            .unwrap();
        let values = TemplateValues {
            date: "2026-10-15".into(),
            time: "12:05".into(),
            title: "{{date}}".into(),
        };
        assert_eq!(
            render_template("{{title}} {{unknown}} {{", &values),
            "{{date}} {{unknown}} {{"
        );
        let path = daily_note(&mut vault, &values).unwrap();
        assert_eq!(
            vault.read(&path).unwrap(),
            "# 2026-10-15\n12:05 · {{date}}\n"
        );
        std::fs::write(directory.path().join(&path), "user edits").unwrap();
        assert_eq!(daily_note(&mut vault, &values).unwrap(), path);
        assert_eq!(vault.read(&path).unwrap(), "user edits");
    }
}
