use std::{collections::BTreeMap, fs, io::Write, path::Path};

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::model::*;

#[derive(Serialize, Deserialize)]
struct NoteHeader {
    id: String,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    status: NoteStatus,
    #[serde(flatten)]
    metadata: BTreeMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
struct TagHeader {
    topic: String,
    #[serde(default)]
    read_on_start: bool,
    #[serde(flatten)]
    metadata: BTreeMap<String, Value>,
}

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::new("invalid_document", message)
}

pub(crate) fn fingerprint(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true)
}

pub(crate) fn timestamp(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|_| Error::new("invalid_input", "Expected an RFC3339 timestamp"))
}

pub(crate) fn date_bound(value: &str) -> Result<DateTime<Utc>> {
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .map(|v| v.and_utc())
            .ok_or_else(|| Error::new("invalid_input", "Invalid UTC date"));
    }
    timestamp(value)
}

pub(crate) fn date_key(note: &Note) -> Result<String> {
    Ok(format!(
        "{}/{}",
        timestamp(&note.created_at)?.format("%Y-%m-%dT%H:%M:%S%.9fZ"),
        note.id
    ))
}

pub(crate) fn slug(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name.as_bytes()[0].is_ascii_lowercase()
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
    {
        return Err(Error::new(
            "invalid_input",
            "Tag names must start with a lowercase ASCII letter and contain only lowercase letters, digits, hyphens, or underscores (up to 128 bytes)",
        ));
    }
    Ok(())
}

pub(crate) fn tag_name(name: &str) -> Result<(Visibility, &str)> {
    let (visibility, bare) = match name.strip_prefix("private:") {
        Some(bare) => (Visibility::Private, bare),
        None => (Visibility::Public, name),
    };
    slug(bare)?;
    Ok((visibility, bare))
}

pub(crate) fn note_id(id: &str) -> Result<()> {
    let parsed = Uuid::parse_str(id).map_err(|_| {
        Error::new(
            "invalid_input",
            "Expected a complete UUID note ID; use list or search to discover IDs",
        )
    })?;
    if parsed.to_string() != id {
        return Err(Error::new(
            "invalid_input",
            "Use the canonical lowercase, hyphenated UUID note ID",
        ));
    }
    Ok(())
}

fn split(text: &str) -> Result<(&str, &str)> {
    let mut lines = text.split_inclusive('\n');
    let first = lines
        .next()
        .ok_or_else(|| invalid("Missing YAML frontmatter"))?;
    if first.trim_end_matches(['\r', '\n']) != "---" {
        return Err(invalid(
            "Start the file with --- followed by YAML frontmatter",
        ));
    }
    let start = first.len();
    let mut offset = start;
    for line in lines {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Ok((&text[start..offset], &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(invalid("Close YAML frontmatter with a line containing ---"))
}

pub(crate) fn parse_note(text: &str, path: &str, physical: Visibility) -> Result<Note> {
    let (yaml, body) = split(text)?;
    let header: NoteHeader =
        noyalib::from_str(yaml).map_err(|e| invalid(format!("Invalid note frontmatter: {e}")))?;
    note_id(&header.id)?;
    if Path::new(path).file_stem().and_then(|p| p.to_str()) != Some(&header.id) {
        return Err(invalid("The note filename must match its frontmatter ID"));
    }
    timestamp(&header.created_at)?;
    timestamp(&header.updated_at)?;
    if body.trim().is_empty() {
        return Err(invalid("A note must have a nonempty Markdown body"));
    }
    let mut visibility = physical;
    for tag in &header.tags {
        if tag_name(tag)?.0 == Visibility::Private {
            visibility = Visibility::Private;
        }
    }
    Ok(Note {
        id: header.id,
        created_at: header.created_at,
        updated_at: header.updated_at,
        tags: header.tags,
        status: header.status,
        body: body.to_owned(),
        visibility,
        revision: fingerprint(text),
        path: path.into(),
        metadata: header.metadata,
    })
}

pub(crate) fn parse_tag(text: &str, name: &str) -> Result<Tag> {
    let (visibility, _) = tag_name(name)?;
    let (yaml, body) = split(text)?;
    let header: TagHeader =
        noyalib::from_str(yaml).map_err(|e| invalid(format!("Invalid tag frontmatter: {e}")))?;
    if header.topic.trim().is_empty() {
        return Err(invalid("Tag frontmatter must contain a nonempty topic"));
    }
    Ok(Tag {
        name: name.into(),
        topic: header.topic,
        read_on_start: header.read_on_start,
        body: body.into(),
        visibility,
        revision: fingerprint(text),
        metadata: header.metadata,
    })
}

fn assemble(yaml: &str, body: &str) -> String {
    format!(
        "---\n{}\n---\n{}{}",
        yaml.trim_end(),
        body,
        if body.ends_with('\n') || body.is_empty() {
            ""
        } else {
            "\n"
        }
    )
}

pub(crate) fn render_note(note: &Note) -> Result<String> {
    let header = NoteHeader {
        id: note.id.clone(),
        created_at: note.created_at.clone(),
        updated_at: note.updated_at.clone(),
        tags: note.tags.clone(),
        status: note.status,
        metadata: note.metadata.clone(),
    };
    let yaml =
        noyalib::to_string(&header).map_err(|e| invalid(format!("Cannot encode note: {e}")))?;
    Ok(assemble(&yaml, &note.body))
}

pub(crate) fn render_tag(input: &CreateTag) -> Result<String> {
    let header = TagHeader {
        topic: input.topic.clone(),
        read_on_start: input.read_on_start,
        metadata: BTreeMap::new(),
    };
    let yaml =
        noyalib::to_string(&header).map_err(|e| invalid(format!("Cannot encode tag: {e}")))?;
    Ok(assemble(&yaml, &input.body))
}

pub(crate) fn patch_note(original: &str, note: &Note, patch: &UpdateNote) -> Result<String> {
    let (yaml, original_body) = split(original)?;
    let mut document = noyalib::cst::parse_document(yaml)
        .map_err(|e| invalid(format!("Cannot edit frontmatter: {e}")))?;
    let mut set = |key: &str, value: Value| -> Result<()> {
        let fragment = serde_json::to_string(&value).map_err(|e| invalid(e.to_string()))?;
        if document.get(key).is_some() {
            document
                .set(key, &fragment)
                .map_err(|e| invalid(format!("Cannot update {key}: {e}")))
        } else {
            document = noyalib::cst::parse_document(&format!("{}\n{key}: {fragment}\n", document))
                .map_err(|e| invalid(format!("Cannot add {key}: {e}")))?;
            Ok(())
        }
    };
    set("updated_at", Value::String(note.updated_at.clone()))?;
    if patch.tags.is_some() {
        set(
            "tags",
            serde_json::to_value(&note.tags).map_err(|e| invalid(e.to_string()))?,
        )?;
    }
    if patch.status.is_some() {
        set(
            "status",
            serde_json::to_value(note.status).map_err(|e| invalid(e.to_string()))?,
        )?;
    }
    let body = patch.body.as_deref().unwrap_or(original_body);
    Ok(assemble(&document.to_string(), body))
}

/// Refuse symlinked ancestors to keep writes within the selected repository.
pub(crate) fn safe_path(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(Error::new(
                "invalid_input",
                "Store paths must remain inside the repository",
            ));
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(Error::new(
                    "io",
                    format!("Refusing symlink at {}", current.display()),
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, text: &str, replace: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("io", "Missing parent directory"))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(text.as_bytes())?;
    temp.as_file().sync_all()?;
    let persisted = if replace {
        temp.persist(path)
    } else {
        temp.persist_noclobber(path)
    };
    persisted
        .map_err(|e| Error::new("io", format!("Cannot save {}: {}", path.display(), e.error)))?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
