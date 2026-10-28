# Source behavior

If index refresh fails after a successful source save, return success with the
saved note ID and warnings. Keep source reads available. Callers can then repair
the disposable cache without repeating creation.

Note edits must retain nested frontmatter metadata and comments, including when
the input uses CRLF line endings. Publishing must retain the private original and
remove additional metadata and private tags from the new public copy.

Require the caller's last-read revision for every edit and publication, including
archive/unarchive and publication previews. Hold the operation lock across the
source read, revision comparison, and save. Reject stale requests before changing
sources. After a conflict, re-read and reconcile; never resubmit a stale body with
a newly fetched revision without reviewing the intervening changes.
