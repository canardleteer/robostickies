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

pub(crate) fn parse_note_for_version(
    text: &str,
    path: &str,
    physical: Visibility,
    version: crate::version::DataVersion,
) -> Result<Note> {
    let note = parse_note(text, path, physical)?;
    if version.requires_valid_tag_indentation() {
        let (yaml, _) = split(text)?;
        let document = noyalib::cst::parse_document(yaml).map_err(yaml_error)?;
        require_valid_tag_indentation(&document)?;
    }
    Ok(note)
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

fn replace_frontmatter(original: &str, yaml: &str, replacement: &str) -> Result<String> {
    let offset = original
        .find('\n')
        .ok_or_else(|| invalid("Missing frontmatter line break"))?
        + 1;
    Ok(format!(
        "{}{}{}",
        &original[..offset],
        replacement,
        &original[offset + yaml.len()..]
    ))
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

// Use the CST's classifications rather than scanning YAML ourselves. Block scalar
// headers and their comments share one token in noyalib 0.0.58; load_comments
// omits those header comments.
fn yaml_tokens(document: &noyalib::cst::Document) -> Vec<(noyalib::cst::SyntaxKind, usize, &str)> {
    fn collect<'a>(
        node: &noyalib::cst::GreenNode,
        source: &'a str,
        mut offset: usize,
        out: &mut Vec<(noyalib::cst::SyntaxKind, usize, &'a str)>,
    ) {
        for child in node.children() {
            match child {
                noyalib::cst::GreenChild::Node(node) => collect(node, source, offset, out),
                noyalib::cst::GreenChild::Token { kind, len } => {
                    out.push((*kind, offset, &source[offset..offset + *len as usize]))
                }
            }
            offset += child.text_len();
        }
    }
    let mut tokens = Vec::new();
    collect(document.syntax(), document.source(), 0, &mut tokens);
    tokens
}

fn yaml_comments(document: &noyalib::cst::Document) -> Vec<String> {
    use noyalib::cst::SyntaxKind;
    yaml_tokens(document)
        .into_iter()
        .filter_map(|(kind, _, text)| match kind {
            SyntaxKind::Comment => Some(text.trim_end_matches('\r').to_owned()),
            SyntaxKind::LiteralScalar | SyntaxKind::FoldedScalar => text
                .lines()
                .next()?
                .split_once('#')
                .map(|(_, body)| format!("#{body}")),
            _ => None,
        })
        .collect()
}

fn yaml_alias(document: &noyalib::cst::Document, path: &str) -> bool {
    use noyalib::cst::SyntaxKind;
    let Some((_, key_end)) = document.key_span(path) else {
        return false;
    };
    // span_at resolves an alias to its anchor; inspect the entry's own token.
    yaml_tokens(document)
        .into_iter()
        .find(|(kind, start, _)| {
            *start >= key_end
                && !matches!(
                    kind,
                    SyntaxKind::ColonIndicator
                        | SyntaxKind::Whitespace
                        | SyntaxKind::Newline
                        | SyntaxKind::Comment
                )
        })
        .is_some_and(|(kind, _, _)| kind == SyntaxKind::AliasMark)
}

fn unindented_flow_sequence(document: &noyalib::cst::Document, path: &str) -> Option<usize> {
    let (start, _) = document.span_at(path)?;
    (document.source().as_bytes().get(start) == Some(&b'[')
        && document.source()[..start].rsplit('\n').next() == Some(""))
    .then_some(start)
}

fn require_valid_tag_indentation(document: &noyalib::cst::Document) -> Result<()> {
    if unindented_flow_sequence(document, "tags").is_some() {
        return Err(invalid(
            "Unindented flow tags require an explicit legacy-store upgrade or a source correction before editing",
        ));
    }
    Ok(())
}

/// Prepare the v1alpha3 source transformation; ordinary edits never call this.
/// Only missing indentation changes. Delimiters, body, and all YAML values survive.
pub(crate) fn migrate_note_v1alpha3(original: &str) -> Result<String> {
    let (yaml, _) = split(original)?;
    let mut document = noyalib::cst::parse_document(yaml).map_err(yaml_error)?;
    let Some(start) = unindented_flow_sequence(&document, "tags") else {
        return Ok(original.to_owned());
    };
    let expected = (*document.as_value()).clone();
    document
        .replace_span(start, start, &" ".repeat(document.indent_unit()))
        .map_err(yaml_error)?;
    if *document.as_value() != expected {
        return Err(invalid("YAML migration changed frontmatter values"));
    }
    require_valid_tag_indentation(&document)?;
    replace_frontmatter(original, yaml, document.source())
}

pub(crate) fn patch_note(original: &str, note: &Note, patch: &UpdateNote) -> Result<String> {
    use noyalib::Value as YamlValue;
    let (yaml, _) = split(original)?;
    let newline = if original.starts_with("---\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut document = noyalib::cst::parse_document(yaml).map_err(yaml_error)?;
    require_valid_tag_indentation(&document)?;
    let comments = yaml_comments(&document);
    let mut set = |key: &str, value: YamlValue| -> Result<()> {
        if value.as_sequence().is_some()
            && yaml_alias(&document, key)
            && document.as_value()[key] != value
        {
            // set_value refuses a collection over an alias token. set's literal
            // fragment replaces only that reference, leaving its anchor intact.
            let config = noyalib::SerializerConfig::new().flow_style(noyalib::FlowStyle::Flow);
            let fragment =
                noyalib::to_string_value_with_config(&value, &config).map_err(yaml_error)?;
            document
                .set(key, fragment.trim_end_matches('\n'))
                .map_err(yaml_error)?;
        } else {
            document.set_path(key, &value).map_err(yaml_error)?;
        }
        // The pinned writer can leave a new empty flow sequence at column zero
        // when replacing an indentless block sequence. Fix only the new output.
        if value.as_sequence().is_some()
            && let Some(start) = unindented_flow_sequence(&document, key)
        {
            document
                .replace_span(start, start, &" ".repeat(document.indent_unit()))
                .map_err(yaml_error)?;
        }
        Ok(())
    };
    set("updated_at", YamlValue::String(note.updated_at.clone()))?;
    if patch.tags.is_some() {
        set("tags", noyalib::to_value(&note.tags).map_err(yaml_error)?)?;
    }
    if patch.status.is_some() {
        set("status", YamlValue::String(note.status.as_str().into()))?;
    }
    if let Some(hint) = &patch.body_format {
        set("body_format", YamlValue::String(hint.clone()))?;
    }
    if patch.clear_body_format && document.get("body_format").is_some() {
        // remove in 0.0.58 can strand comment-shaped block scalar content as
        // YAML comments. Empty the value first; the CST owns both operations.
        document
            .set_value("body_format", &YamlValue::Null)
            .map_err(yaml_error)?;
        document.remove("body_format").map_err(yaml_error)?;
    }
    let mut retained = yaml_comments(&document);
    let mut displaced = String::new();
    for comment in comments {
        if let Some(index) = retained.iter().position(|text| *text == comment) {
            retained.remove(index);
        } else {
            displaced.push_str(&comment);
            displaced.push_str(newline);
        }
    }
    if !displaced.is_empty() {
        let end = document.source().len();
        let separator = if document.source().ends_with('\n') {
            ""
        } else {
            newline
        };
        document
            .replace_span(end, end, &format!("{separator}{displaced}"))
            .map_err(yaml_error)?;
    }
    if let Some(body) = &patch.body {
        let body = body.replace("\r\n", "\n").replace('\n', newline);
        Ok(assemble_with_newline(document.source(), &body, newline))
    } else {
        replace_frontmatter(original, yaml, document.source())
    }
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
                // set_path inserts missing entries before the mapping's trailing
                // blank lines; both the metadata and that separation survive.
                assert!(edited.contains(metadata.trim_end_matches(['\r', '\n'])));
                assert!(split(&edited).unwrap().0.ends_with(&newline.repeat(2)));
                let read = parse_note(&edited, &note.path, Visibility::Private).unwrap();
                assert_eq!(read.metadata, note.metadata);
                assert_eq!(read.tags, note.tags);
                assert_eq!(read.body, format!("First{newline}Second{newline}"));
            }
        }
    }

    #[test]
    fn edits_retain_comments_from_replaced_and_cleared_fields() {
        use saphyr::LoadableYamlNode;

        for newline in ["\n", "\r\n"] {
            for clear_hint in [false, true] {
                for tags in [vec!["resolved".to_owned()], vec![]] {
                    let original = source()
                        .replace(
                            "updated_at: 2026-01-01T00:00:00Z",
                            "updated_at: |- # timestamp\n  2026-01-01T00:00:00Z",
                        )
                        .replace(
                            "---\nBody",
                            concat!(
                                "tags:\n  - papercut #duplicate\n  - paperclip #duplicate\n",
                                "status: >- # status\n  active\n",
                                "# hint heading\nbody_format: |- # hint header\n  markdown\n",
                                "extra:\n  literal: | # metadata header\n    # literal data\n",
                                "---\nBody",
                            ),
                        )
                        .replace('\n', newline);
                    let mut note =
                        parse_note(&original, &format!("notes/{ID}.md"), Visibility::Public)
                            .unwrap();
                    note.updated_at = "2026-11-08T01:02:03Z".into();
                    note.tags = tags;
                    note.status = NoteStatus::Archived;
                    let patch = UpdateNote {
                        tags: Some(note.tags.clone()),
                        status: Some(note.status),
                        body_format: (!clear_hint).then(|| "text".into()),
                        clear_body_format: clear_hint,
                        ..UpdateNote::default()
                    };
                    let edited = patch_note(&original, &note, &patch).unwrap();
                    for comment in [
                        "# timestamp",
                        "#duplicate",
                        "# status",
                        "# hint heading",
                        "# hint header",
                        "# metadata header",
                    ] {
                        assert_eq!(
                            edited.matches(comment).count(),
                            original.matches(comment).count(),
                            "Lost {comment}: {edited}"
                        );
                    }
                    let footer = format!(
                        "# timestamp{newline}#duplicate{newline}# status{newline}{}# hint header{newline}",
                        if clear_hint {
                            format!("# hint heading{newline}")
                        } else {
                            String::new()
                        }
                    );
                    assert!(split(&edited).unwrap().0.ends_with(&footer));
                    assert!(
                        edited.contains(
                            &"extra:\n  literal: | # metadata header\n    # literal data\n"
                                .replace('\n', newline)
                        )
                    );
                    let read = parse_note(&edited, &note.path, Visibility::Public).unwrap();
                    assert_eq!(read.updated_at, note.updated_at);
                    assert_eq!(read.tags, note.tags);
                    assert_eq!(read.status, note.status);
                    assert_eq!(read.metadata["extra"], note.metadata["extra"]);
                    assert_eq!(
                        read.metadata.get("body_format"),
                        (!clear_hint).then_some(&Value::String("text".into()))
                    );
                    assert_eq!(read.body, note.body);
                    saphyr::Yaml::load_from_str(split(&edited).unwrap().0).unwrap();
                    if newline == "\r\n" {
                        assert!(!edited.replace("\r\n", "").contains('\n'));
                    }
                }
            }
        }
    }

    #[test]
    fn tag_alias_updates_detach_only_the_edited_reference() {
        for newline in ["\n", "\r\n"] {
            for tags in [vec!["papercut".into()], vec!["resolved".into()], vec![]] {
                let original = source().replace("---\nBody", "shared: &tag_list [papercut]\ntags: *tag_list # alias comment\nother: *tag_list\n---\nBody").trim_end_matches('\n').replace('\n', newline);
                let mut note =
                    parse_note(&original, &format!("notes/{ID}.md"), Visibility::Public).unwrap();
                note.tags = tags;
                let patch = UpdateNote {
                    tags: Some(note.tags.clone()),
                    ..UpdateNote::default()
                };
                let edited = patch_note(&original, &note, &patch).unwrap();
                let read = parse_note(&edited, &note.path, Visibility::Public).unwrap();
                assert_eq!(read.tags, note.tags);
                assert_eq!(read.metadata, note.metadata);
                assert_eq!(read.body, note.body);
                assert!(edited.contains("shared: &tag_list [papercut]"));
                assert!(edited.contains("other: *tag_list"));
                assert!(edited.contains("# alias comment"));
                assert_eq!(
                    edited.contains("tags: *tag_list"),
                    note.tags == ["papercut"]
                );
            }
        }
    }

    #[test]
    fn cleared_scalar_content_is_not_restored_as_a_comment() {
        for hint in [
            "\"hash # scalar content\"",
            "|- # hint header\n  # scalar content",
        ] {
            let original =
                source().replace("---\nBody", &format!("body_format: {hint}\n---\nBody"));
            let note =
                parse_note(&original, &format!("notes/{ID}.md"), Visibility::Public).unwrap();
            let edited = patch_note(
                &original,
                &note,
                &UpdateNote {
                    clear_body_format: true,
                    ..UpdateNote::default()
                },
            )
            .unwrap();
            assert!(!edited.contains("# scalar content"), "{edited}");
            assert_eq!(edited.contains("# hint header"), hint.starts_with('|'));
        }
    }

    #[test]
    fn tag_updates_remain_portable_yaml_and_preserve_source_context() {
        use saphyr::{LoadableYamlNode, Yaml};

        for newline in ["\n", "\r\n"] {
            for tags in [
                "tags: # keep tag heading\n  - papercut # keep tag comment\n",
                "tags:\n- papercut # keep tag comment\n",
                "tags: # keep tag heading\n  - papercut # keep tag comment\n  # keep between tags\n  - paperclip # keep second tag comment\n",
                "tags:\n  - papercut # keep tag comment\n  - paperclip # keep tag comment\n",
                "tags: [papercut] # keep tag heading\n",
                "tags: [papercut, # keep between tags\n  paperclip] # keep tag heading\n",
                "tags: [] # keep tag heading\n",
                "",
            ] {
                for updated_tags in [vec!["papercut", "resolved"], vec!["null", "true"], vec![]] {
                    let original = source()
                        .replace(
                            "---\nBody",
                            &format!(
                                "{tags}status: active # keep status\nextra:\n  nested: [one, two] # keep metadata\n---\nBody"
                            ),
                        )
                        .replace('\n', newline);
                    let mut note = parse_note(
                        &original,
                        &format!("private/notes/{ID}.md"),
                        Visibility::Private,
                    )
                    .unwrap();
                    note.tags = updated_tags.iter().map(|tag| (*tag).into()).collect();
                    note.status = NoteStatus::Archived;
                    let patch = UpdateNote {
                        tags: Some(note.tags.clone()),
                        status: Some(note.status),
                        ..UpdateNote::default()
                    };
                    let edited = patch_note(&original, &note, &patch).unwrap();
                    let (header, _) = split(&edited).unwrap();
                    let (old_header, _) = split(&original).unwrap();
                    let comments = |text: &str| {
                        let mut values: Vec<_> = noyalib::load_comments(text)
                            .unwrap()
                            .into_iter()
                            .map(|comment| comment.text)
                            .collect();
                        values.sort();
                        values
                    };
                    assert_eq!(comments(header), comments(old_header));
                    let docs = Yaml::load_from_str(header)
                        .unwrap_or_else(|error| panic!("Nonportable YAML: {error}\n{edited}"));
                    let values: Vec<_> = docs[0]["tags"]
                        .as_sequence()
                        .unwrap()
                        .iter()
                        .map(|value| value.as_str().unwrap())
                        .collect();
                    assert_eq!(values, updated_tags);
                    assert_eq!(docs[0]["status"].as_str(), Some("archived"));
                    let read = parse_note(&edited, &note.path, Visibility::Private).unwrap();
                    assert_eq!(read.metadata, note.metadata);
                    assert_eq!(read.body, note.body);
                    assert_eq!(read.visibility, Visibility::Private);
                    for comment in [
                        "# keep tag heading",
                        "# keep tag comment",
                        "# keep between tags",
                        "# keep second tag comment",
                        "# keep status",
                        "# keep metadata",
                    ] {
                        if original.contains(comment) {
                            assert!(edited.contains(comment), "Lost {comment}: {edited}");
                        }
                    }
                    if newline == "\r\n" {
                        assert!(!edited.replace("\r\n", "").contains('\n'));
                    }
                }
            }
        }
    }

    #[test]
    fn legacy_unindented_flow_tags_require_explicit_migration() {
        use saphyr::{LoadableYamlNode, Yaml};

        for newline in ["\n", "\r\n"] {
            for tags in ["[\"papercut\",\"resolved\"]", "[]"] {
                let original = source()
                    .replace("---\nBody", &format!("tags:\n{tags}\nstatus: active\nextra: {{nested: [1, 2]}} # retain\n---\nBody"))
                    .replace('\n', newline);
                let note =
                    parse_note(&original, &format!("notes/{ID}.md"), Visibility::Public).unwrap();
                let patch = UpdateNote {
                    body: Some("Updated body\n".into()),
                    ..UpdateNote::default()
                };
                assert_eq!(
                    patch_note(&original, &note, &patch).unwrap_err().code,
                    "invalid_document"
                );
                let migrated = migrate_note_v1alpha3(&original).unwrap();
                assert_eq!(migrate_note_v1alpha3(&migrated).unwrap(), migrated);
                let read = parse_note_for_version(
                    &migrated,
                    &note.path,
                    Visibility::Public,
                    crate::version::DataVersion::V1Alpha3,
                )
                .unwrap();
                assert_eq!(read.body, note.body);
                assert_eq!(read.updated_at, note.updated_at);
                assert_ne!(read.revision, note.revision);
                let edited = patch_note(&migrated, &read, &patch).unwrap();
                let (header, _) = split(&edited).unwrap();
                assert!(Yaml::load_from_str(header).is_ok(), "{edited}");
                let read = parse_note(&edited, &note.path, Visibility::Public).unwrap();
                assert_eq!(read.tags, note.tags);
                assert_eq!(read.status, note.status);
                assert_eq!(read.metadata, note.metadata);
                assert!(edited.contains("# retain"));
                assert_eq!(read.body, format!("Updated body{newline}"));
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
