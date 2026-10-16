//! YAML frontmatter edits. Markdown bodies and untouched property source stay intact.
pub use serde_yaml_ng as yaml;
use std::ops::Range;
use yaml::{Mapping, Value};

pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug)]
pub struct Frontmatter {
    pub range: Range<usize>,
    pub yaml: Range<usize>,
}

pub fn frontmatter(source: &str) -> Result<Option<Frontmatter>> {
    let start = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let mut lines = source[start..].split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Ok(None);
    };
    if first.trim_end() != "---" {
        return Ok(None);
    }
    let yaml_start = start + first.len();
    let mut offset = yaml_start;
    for line in lines {
        if matches!(line.trim_end(), "---" | "...") {
            return Ok(Some(Frontmatter {
                range: start..offset + line.len(),
                yaml: yaml_start..offset,
            }));
        }
        offset += line.len();
    }
    Err("The YAML properties block is missing its closing --- line.".into())
}

pub fn parse(yaml: &str) -> Result<Mapping> {
    match serde_yaml_ng::from_str::<Value>(yaml).map_err(|e| e.to_string())? {
        Value::Mapping(map) if map.keys().all(|k| k.as_str().is_some()) => Ok(map),
        Value::Null => Ok(Mapping::new()),
        _ => Err("Properties must be a YAML mapping with text keys.".into()),
    }
}

pub fn read(source: &str) -> Result<Mapping> {
    match frontmatter(source)? {
        Some(header) => parse(&source[header.yaml]),
        None => Ok(Mapping::new()),
    }
}

pub fn value_text(value: &Value) -> String {
    if let Value::Sequence(values) = value {
        let items: Vec<_> = values
            .iter()
            .map(|value| {
                serde_yaml_ng::to_string(value)
                    .unwrap_or_default()
                    .trim_end()
                    .to_owned()
            })
            .collect();
        if values
            .iter()
            .all(|v| !matches!(v, Value::Sequence(_) | Value::Mapping(_)))
            && items.iter().all(|s| !s.contains('\n'))
        {
            return format!("[{}]", items.join(", "));
        }
    }

    serde_yaml_ng::to_string(value)
        .unwrap_or_default()
        .trim_end()
        .to_owned()
}

pub fn tags(source: &str) -> Vec<String> {
    let Ok(map) = read(source) else { return vec![] };
    let Some(value) = map.get(Value::String("tags".into())) else {
        return vec![];
    };
    let strings: Vec<&str> = match value {
        Value::String(s) => vec![s],
        Value::Sequence(values) => values.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    let mut tags: Vec<_> = strings
        .into_iter()
        .flat_map(|s| s.split([',', ' ', '\t', '\n']))
        .map(|s| s.strip_prefix('#').unwrap_or(s))
        .filter(|s| crate::markdown::valid_tag(s))
        .map(str::to_owned)
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

/// Validate before replacing only the header. Existing body bytes never change.
pub fn replace_yaml(source: &str, yaml: &str) -> Result<(Range<usize>, String)> {
    parse(yaml)?;
    let header = frontmatter(source)?;
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let yaml = yaml.replace("\r\n", "\n").replace('\n', newline);
    let start = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let range = header.map_or(start..start, |h| h.range);
    Ok((
        range,
        format!(
            "---{newline}{}{newline}---{newline}",
            yaml.trim_end_matches(['\n', '\r'])
        ),
    ))
}

fn line_key(line: &str) -> Option<String> {
    if line.starts_with(char::is_whitespace) || line.starts_with('#') {
        return None;
    }
    for (i, _) in line.match_indices(':') {
        if !line[i + 1..].starts_with(char::is_whitespace) && !line[i + 1..].is_empty() {
            continue;
        }
        let candidate = format!("{}: null", &line[..i]);
        if let Ok(map) = parse(&candidate)
            && map.len() == 1
        {
            return map.keys().next().and_then(Value::as_str).map(str::to_owned);
        }
    }
    None
}

/// Patch an individual property while preserving unrelated YAML (including comments).
/// Unusual YAML layouts remain editable in the raw YAML editor.
pub fn set(source: &str, key: &str, value: Option<Value>) -> Result<String> {
    if key.trim().is_empty() {
        return Err("Enter a property name.".into());
    }
    let mut expected = read(source)?;
    let yaml_key = Value::String(key.into());
    let old = expected.get(&yaml_key);
    if old == value.as_ref() {
        return Ok(source.into());
    }
    let existed = old.is_some();
    match &value {
        Some(value) => {
            expected.insert(yaml_key.clone(), value.clone());
        }
        None => {
            expected.remove(&yaml_key);
        }
    }
    let Some(header) = frontmatter(source)? else {
        let yaml = serde_yaml_ng::to_string(&expected).map_err(|e| e.to_string())?;
        let (range, replacement) = replace_yaml(source, &yaml)?;
        let mut output = source.to_owned();
        output.replace_range(range, &replacement);
        return Ok(output);
    };
    let yaml = &source[header.yaml.clone()];
    let mut keys = vec![];
    let mut offset = 0;
    for line in yaml.split_inclusive('\n') {
        if let Some(key) = line_key(line) {
            keys.push((offset, key));
        }
        offset += line.len();
    }
    let range = if existed {
        let index = keys
            .iter()
            .position(|(_, k)| k == key)
            .ok_or("This YAML layout needs the YAML editor to preserve its structure.")?;
        let start = keys[index].0;
        let mut end = keys
            .get(index + 1)
            .map_or(yaml.len(), |(offset, _)| *offset);
        // Keep comments/blank lines preceding the next property.
        for line in yaml[start..end].split_inclusive('\n').rev() {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                end -= line.len();
            } else {
                break;
            }
        }
        start..end
    } else {
        yaml.len()..yaml.len()
    };
    let replacement = match value {
        Some(value) => {
            let mut one = Mapping::new();
            one.insert(yaml_key, value);
            let text = serde_yaml_ng::to_string(&one).map_err(|e| e.to_string())?;
            if source.contains("\r\n") {
                text.replace('\n', "\r\n")
            } else {
                text
            }
        }
        None => String::new(),
    };
    let mut output = source.to_owned();
    output.replace_range(
        header.yaml.start + range.start..header.yaml.start + range.end,
        &replacement,
    );
    if read(&output)? != expected {
        return Err(
            "This change affects YAML aliases or structure. Use the YAML editor to review it."
                .into(),
        );
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn property_edits_preserve_comments_body_types_and_line_endings() {
        let source = "\u{feff}---\r\n# keep\r\ntitle: 'Old'\r\n\r\n# tags below\r\ntags: [rust, 'work/notes']\r\nextra:\r\n  nested: true\r\n---\r\n\r\n# Body\r\n";
        let changed = set(source, "title", Some(Value::String("A: new title".into()))).unwrap();
        assert!(changed.contains("# keep\r\n"));
        assert!(changed.ends_with("---\r\n\r\n# Body\r\n"));
        assert!(
            changed
                .contains("# tags below\r\ntags: [rust, 'work/notes']\r\nextra:\r\n  nested: true")
        );
        assert_eq!(
            read(&changed).unwrap()["title"].as_str(),
            Some("A: new title")
        );
        assert_eq!(tags(&changed), ["rust", "work/notes"]);
        let changed = set(&changed, "title", None).unwrap();
        assert!(
            !read(&changed)
                .unwrap()
                .contains_key(Value::String("title".into()))
        );
        assert!(changed.contains("# tags below"));
        let created = set("Body", "done", Some(Value::Bool(false))).unwrap();
        assert!(created.ends_with("---\nBody"));
        assert_eq!(read(&created).unwrap()["done"], Value::Bool(false));
    }
    #[test]
    fn invalid_yaml_and_alias_changes_never_silently_destroy_properties() {
        assert!(read("---\nx: [\n---\nbody").is_err());
        assert!(read("---\nx: 1").is_err());
        assert!(parse("x: 1\nx: 2").is_err());
        assert!(parse("[one, two]").is_err());
        assert!(
            set(
                "---\nx: &a 1\ny: *a\n---\nbody",
                "x",
                Some(Value::Bool(true))
            )
            .is_err()
        );
        let source = "---\n{x: 1, y: 2}\n---\nbody";
        assert!(set(source, "x", None).is_err());
        let (range, text) = replace_yaml(source, "x: 2\ny: [a, b]").unwrap();
        assert_eq!(&source[range.end..], "body");
        assert_eq!(read(&text).unwrap()["x"].as_u64(), Some(2));
    }
}
