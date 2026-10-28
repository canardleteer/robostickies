# Source behavior

If index refresh fails after a successful source save, return success with the
saved note ID and warnings. Keep source reads available. Callers can then repair
the disposable cache without repeating creation.

Note edits must retain nested frontmatter metadata and comments, including when
the input uses CRLF line endings. Check the latest revision before applying a
guarded edit. Publishing must retain the private original and remove additional
metadata and private tags from the new public copy.
