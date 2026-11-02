//! Build-time source identity. Only the package's own checkout may supply Git data.

use std::{fs, io, path::Path};

use built::gix;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Source {
    pub(crate) commit: Option<String>,
    pub(crate) dirty: Option<bool>,
}

impl Source {
    pub(crate) fn state(&self) -> &'static str {
        match self.dirty {
            Some(false) => "clean",
            Some(true) => "dirty",
            None => "unknown",
        }
    }
}

pub(crate) fn generate(manifest: &Path, output: &Path) -> io::Result<()> {
    // Passing None prevents built from discovering an enclosing consumer checkout.
    built::write_built_file_with_opts(None, &output.join("built.rs"))?;
    let (source, watches) = source(manifest);
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_support");
    for path in watches {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    fs::write(
        output.join("source.rs"),
        format!(
            "pub const COMMIT: &str = {:?};\npub const SOURCE_STATE: &str = {:?};\n",
            source.commit.as_deref().unwrap_or("unknown"),
            source.state(),
        ),
    )
}

pub(crate) fn source(manifest: &Path) -> (Source, Vec<std::path::PathBuf>) {
    let vcs = manifest.join(".cargo_vcs_info.json");
    if vcs.exists() {
        // Packaged provenance takes precedence even inside an unrelated Git tree.
        let source = fs::read(&vcs)
            .ok()
            .and_then(|bytes| packaged(&bytes))
            .unwrap_or_default();
        return (source, vec![vcs]);
    }
    // Open exactly this directory; never search ancestors or honor GIT_DIR.
    let Ok(repo) = gix::open(manifest) else {
        return (Source::default(), vec![manifest.join(".git")]);
    };
    let mut watches = vec![
        manifest.join(".git"),
        repo.path().into(),
        repo.common_dir().into(),
    ];
    // gix 0.83 can panic below the SHA-1 index header/checksum minimum.
    // Source identification remains optional when that index is truncated.
    let truncated_index = fs::metadata(repo.index_path()).is_ok_and(|file| file.len() < 32);
    if !truncated_index && let Ok(index) = repo.index() {
        watches.extend(
            index
                .entries()
                .iter()
                .map(|entry| manifest.join(gix::path::from_bstr(entry.path(&index)))),
        );
    }
    let source = Source {
        commit: repo.head_id().ok().map(|id| id.to_string()),
        dirty: if truncated_index {
            None
        } else {
            repo.is_dirty().ok()
        },
    };
    watches.sort();
    watches.dedup();
    (source, watches)
}

fn packaged(bytes: &[u8]) -> Option<Source> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let git = &value["git"];
    let commit = git["sha1"].as_str()?;
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let dirty = match git.get("dirty") {
        Some(value) => value.as_bool()?,
        None => false,
    };
    Some(Source {
        commit: Some(commit.into()),
        dirty: Some(dirty),
    })
}
