use std::{collections::BTreeMap, fs, io::Write, path::Path};

use chrono::{DateTime, NaiveDate, NaiveTime, SecondsFormat, Utc};
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

fn yaml_error(error: noyalib::Error) -> Error {
    invalid(format!("Invalid YAML frontmatter: {error}"))
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
        return Ok(date.and_time(NaiveTime::MIN).and_utc());
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
    let header: NoteHeader = noyalib::from_str(yaml).map_err(yaml_error)?;
    note_id(&header.id)?;
    if Path::new(path).file_stem().and_then(|p| p.to_str()) != Some(&header.id) {
        return Err(invalid("The note filename must match its frontmatter ID"));
    }
    timestamp(&header.created_at)?;
    timestamp(&header.updated_at)?;
    if body.trim().is_empty() {
        return Err(invalid("A note must have a nonempty UTF-8 text body"));
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
    let header: TagHeader = noyalib::from_str(yaml).map_err(yaml_error)?;
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
    assemble_with_newline(yaml, body, "\n")
}

fn assemble_with_newline(yaml: &str, body: &str, newline: &str) -> String {
    format!(
        "---{newline}{yaml}{}---{newline}{}{}",
        if yaml.ends_with('\n') { "" } else { newline },
        body,
        if body.ends_with('\n') || body.is_empty() {
            ""
        } else {
            newline
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
    let yaml = noyalib::to_string(&header).map_err(yaml_error)?;
    Ok(assemble(&yaml, &note.body))
}

pub(crate) fn render_tag(input: &CreateTag) -> Result<String> {
    let header = TagHeader {
        topic: input.topic.clone(),
        read_on_start: input.read_on_start,
        metadata: BTreeMap::new(),
    };
    let yaml = noyalib::to_string(&header).map_err(yaml_error)?;
    Ok(assemble(&yaml, &input.body))
}

pub(crate) fn patch_note(original: &str, note: &Note, patch: &UpdateNote) -> Result<String> {
    let (yaml, original_body) = split(original)?;
    let newline = if original.starts_with("---\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut document = noyalib::cst::parse_document(yaml).map_err(yaml_error)?;
    let mut set = |key: &str, value: Value| -> Result<()> {
        let fragment = value.to_string();
        if document.get(key).is_some() {
            document.set(key, &fragment).map_err(yaml_error)
        } else {
            let yaml = document.to_string();
            let separator = if yaml.ends_with('\n') { "" } else { newline };
            document = noyalib::cst::parse_document(&format!(
                "{yaml}{separator}{key}: {fragment}{newline}"
            ))
            .map_err(yaml_error)?;
            Ok(())
        }
    };
    set("updated_at", Value::String(note.updated_at.clone()))?;
    if patch.tags.is_some() {
        set(
            "tags",
            Value::Array(note.tags.iter().cloned().map(Value::String).collect()),
        )?;
    }
    if patch.status.is_some() {
        set("status", Value::String(note.status.as_str().into()))?;
    }
    if let Some(hint) = &patch.body_format {
        set("body_format", Value::String(hint.clone()))?;
    }
    if patch.clear_body_format && document.get("body_format").is_some() {
        document.remove("body_format").map_err(yaml_error)?;
    }
    let body = patch
        .body
        .as_deref()
        .map(|body| body.replace("\r\n", "\n").replace('\n', newline));
    Ok(assemble_with_newline(
        &document.to_string(),
        body.as_deref().unwrap_or(original_body),
        newline,
    ))
}

pub(crate) fn set_body_format(
    metadata: &mut BTreeMap<String, Value>,
    hint: Option<&str>,
    clear: bool,
) -> Result<()> {
    if hint.is_some() && clear {
        return Err(Error::new(
            "invalid_input",
            "body_format and clear_body_format conflict",
        ));
    }
    if hint.is_none() && !clear {
        return Ok(());
    }
    if hint.is_some_and(|value| value.trim().is_empty()) {
        return Err(Error::new(
            "invalid_input",
            "body_format must be a nonblank string",
        ));
    }
    if metadata
        .get("body_format")
        .is_some_and(|value| !value.is_string())
    {
        return Err(Error::new(
            "invalid_input",
            "Existing body_format metadata is not a string; reconcile it in the source before setting or clearing the hint",
        ));
    }
    if let Some(hint) = hint {
        metadata.insert("body_format".into(), Value::String(hint.into()));
    } else {
        metadata.remove("body_format");
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "00000000-0000-0000-0000-000000000001";

    fn source() -> String {
        format!(
            "---\nid: {ID}\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\nBody\n"
        )
    }

    #[test]
    fn malformed_documents_and_noncanonical_ids_have_useful_errors() {
        for text in [
            "",
            "body",
            "---\nunterminated",
            "---\ninvalid: [\n---\nbody",
        ] {
            assert!(parse_note(text, &format!("notes/{ID}.md"), Visibility::Public).is_err());
        }
        assert!(parse_note(&source(), "notes/other.md", Visibility::Public).is_err());
        assert!(
            parse_note(
                &source().replace("Body", " "),
                &format!("notes/{ID}.md"),
                Visibility::Public
            )
            .is_err()
        );
        assert!(
            parse_note(
                &source().replace("---\nBody", "tags: ['Invalid']\n---\nBody"),
                &format!("notes/{ID}.md"),
                Visibility::Public
            )
            .is_err()
        );
        assert!(
            parse_note(
                &source().replace("2026-01-01T00:00:00Z", "bad date"),
                &format!("notes/{ID}.md"),
                Visibility::Public
            )
            .is_err()
        );
        for text in ["---\ntopic: [\n---\n", "---\ntopic: ' '\n---\n"] {
            assert_eq!(
                parse_tag(text, "tools").unwrap_err().code,
                "invalid_document"
            );
        }
        assert!(note_id("00000000000000000000000000000001").is_err());
        assert!(note_id("00000000-0000-0000-0000-00000000000A").is_err());
        for tag in ["", "A", "1start", "a/b", &"a".repeat(129)] {
            assert!(slug(tag).is_err());
        }
        assert!(slug("a-1_b").is_ok());
        assert_eq!(
            date_bound("2026-01-01").unwrap(),
            timestamp("2026-01-01T00:00:00Z").unwrap()
        );
    }

    #[test]
    fn patch_backfills_optional_fields_without_losing_metadata_or_body() {
        let original = source().replace("---\nBody", "extra: {nested: [1, 2]} # retain\n---\nBody");
        let mut note =
            parse_note(&original, &format!("notes/{ID}.md"), Visibility::Public).unwrap();
        note.tags = vec!["tools".into()];
        note.status = NoteStatus::Archived;
        let patch = UpdateNote {
            tags: Some(note.tags.clone()),
            status: Some(note.status),
            ..UpdateNote::default()
        };
        let edited = patch_note(&original, &note, &patch).unwrap();
        let read = parse_note(&edited, &note.path, Visibility::Public).unwrap();
        assert_eq!(read.tags, note.tags);
        assert_eq!(read.status, NoteStatus::Archived);
        assert_eq!(read.metadata, note.metadata);
        assert_eq!(read.body, note.body);
        assert!(edited.contains("# retain"));
        assert!(patch_note("broken", &note, &patch).is_err());
        assert!(patch_note("---\ninvalid: [\n---\nbody", &note, &patch).is_err());
        assert_eq!(assemble("topic: test\n", ""), "---\ntopic: test\n---\n");
        assert_eq!(
            assemble("topic: test", "body"),
            "---\ntopic: test\n---\nbody\n"
        );
    }

    #[test]
    fn edits_preserve_newline_style_and_trailing_frontmatter_comments() {
        for newline in ["\n", "\r\n"] {
            for optional_fields in [false, true] {
                let metadata = "extra:\n  nested: [1, {key: value}] # keep metadata\n\n"
                    .replace('\n', newline);
                let fields = if optional_fields {
                    "tags: [paperclip]\nstatus: active # keep status\n"
                } else {
                    ""
                };
                let original = source()
                    .replace("---\nBody", &format!("{fields}{metadata}---\nBody"))
                    .replace("\r\n", "\n")
                    .replace('\n', newline);
                let mut note = parse_note(
                    &original,
                    &format!("private/notes/{ID}.md"),
                    Visibility::Private,
                )
                .unwrap();
                note.tags = vec!["tools".into()];
                note.status = NoteStatus::Archived;
                let patch = UpdateNote {
                    body: Some("First\nSecond\n".into()),
                    tags: Some(note.tags.clone()),
                    status: Some(note.status),
                    ..UpdateNote::default()
                };
                let edited = patch_note(&original, &note, &patch).unwrap();
                assert!(edited.starts_with(&format!("---{newline}")));
                if newline == "\r\n" {
                    assert!(!edited.replace("\r\n", "").contains('\n'));
                }
                assert!(edited.contains(&metadata));
                let read = parse_note(&edited, &note.path, Visibility::Private).unwrap();
                assert_eq!(read.metadata, note.metadata);
                assert_eq!(read.tags, note.tags);
                assert_eq!(read.body, format!("First{newline}Second{newline}"));
            }
        }
    }

    #[test]
    fn atomic_saves_reject_conflicts_and_invalid_parent_paths() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file.md");
        atomic_write(&path, "original", false).unwrap();
        assert_eq!(
            atomic_write(&path, "replacement", false).unwrap_err().code,
            "io"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        atomic_write(&path, "replacement", true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
        assert_eq!(
            atomic_write(Path::new(""), "text", true).unwrap_err().code,
            "io"
        );
        assert!(atomic_write(&path.join("child"), "text", true).is_err());
        assert!(safe_path(temp.path(), Path::new("../escape")).is_err());
        assert!(safe_path(temp.path(), &path).is_err());
        #[cfg(unix)]
        assert!(safe_path(temp.path(), Path::new("file.md/child")).is_err());
    }
}
