use std::{
    fs::{self, File},
    io::{self, ErrorKind, Read as _},
    path::{Path, PathBuf},
};

pub const INCLUDE_KEY: &str = "include";
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

const MAX_INCLUDE_DEPTH: usize = 16;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigSource {
    pub source: String,
    pub files: Vec<PathBuf>,
}

pub fn read_config_source(path: &Path) -> io::Result<String> {
    let file = File::open(path)?;
    let byte_limit = u64::try_from(MAX_CONFIG_BYTES).unwrap_or(u64::MAX - 1);
    let mut source = String::new();
    file.take(byte_limit + 1).read_to_string(&mut source)?;
    if source.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    Ok(source)
}

pub fn expand_config_file(path: &Path) -> io::Result<ConfigSource> {
    let mut expanded = ConfigSource::default();
    let mut stack = Vec::new();
    expand_file(path, 0, &mut stack, &mut expanded)?;
    Ok(expanded)
}

fn expand_file(
    path: &Path,
    depth: usize,
    stack: &mut Vec<PathBuf>,
    expanded: &mut ConfigSource,
) -> io::Result<()> {
    if depth > MAX_INCLUDE_DEPTH {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "configuration include depth exceeds {MAX_INCLUDE_DEPTH} at {}",
                path.display()
            ),
        ));
    }
    let source = read_config_source(path)?;
    let canonical = fs::canonicalize(path)?;
    if stack.contains(&canonical) {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration include cycle at {}", path.display()),
        ));
    }
    stack.push(canonical);
    expanded.files.push(path.to_path_buf());

    for raw_line in source.lines() {
        let Some((optional, target)) = include_target(raw_line)? else {
            append_line(&mut expanded.source, raw_line)?;
            continue;
        };
        let included = resolve_include_path(path, target);
        if optional
            && matches!(fs::metadata(&included), Err(error) if error.kind() == ErrorKind::NotFound)
        {
            expanded.files.push(included);
            continue;
        }
        expand_file(&included, depth + 1, stack, expanded).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("configuration include {}: {error}", included.display()),
            )
        })?;
    }

    stack.pop();
    Ok(())
}

fn append_line(output: &mut String, line: &str) -> io::Result<()> {
    if output.len().saturating_add(line.len()).saturating_add(1) > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration includes exceed the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    output.push_str(line);
    output.push('\n');
    Ok(())
}

pub fn declared_include_paths(path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    collect_paths(path, 0, &mut Vec::new(), &mut paths);
    paths
}

fn collect_paths(path: &Path, depth: usize, stack: &mut Vec<PathBuf>, paths: &mut Vec<PathBuf>) {
    paths.push(path.to_path_buf());
    if depth > MAX_INCLUDE_DEPTH {
        return;
    }
    let Ok(canonical) = fs::canonicalize(path) else {
        return;
    };
    if stack.contains(&canonical) {
        return;
    }
    let Ok(source) = read_config_source(path) else {
        return;
    };
    stack.push(canonical);
    for line in source.lines() {
        if let Ok(Some((_, target))) = include_target(line) {
            collect_paths(&resolve_include_path(path, target), depth + 1, stack, paths);
        }
    }
    stack.pop();
}

fn include_target(line: &str) -> io::Result<Option<(bool, &str)>> {
    let line = line.trim();
    if line.starts_with('#') {
        return Ok(None);
    }
    let Some((key, value)) = line.split_once('=') else {
        return Ok(None);
    };
    if key.trim() != INCLUDE_KEY {
        return Ok(None);
    }
    let value = config_value_without_comment(value).trim();
    let (optional, value) = match value.strip_prefix('-') {
        Some(rest) => (true, rest.trim()),
        None => (false, value),
    };
    let value = unquote(value);
    if value.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "configuration `include` needs a file path",
        ));
    }
    Ok(Some((optional, value)))
}

fn resolve_include_path(current: &Path, target: &str) -> PathBuf {
    let candidate = Path::new(target);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }
    match current.parent() {
        Some(parent) => parent.join(candidate),
        None => candidate.to_path_buf(),
    }
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    match bytes.first() {
        Some(quote @ (b'\'' | b'"')) if bytes.len() >= 2 && bytes.last() == Some(quote) => {
            &value[1..value.len() - 1]
        }
        _ => value,
    }
}

pub(crate) fn config_value_without_comment(value: &str) -> &str {
    config_comment_start(value).map_or(value, |index| &value[..index])
}

pub(crate) fn config_comment_start(value: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut saw_value = false;
    let mut previous_was_whitespace = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            saw_value |= !character.is_whitespace();
            previous_was_whitespace = character.is_whitespace();
            continue;
        }
        match character {
            '\\' if quote.is_some() => escaped = true,
            '"' | '\'' if quote == Some(character) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(character),
            '#' if quote.is_none() && saw_value && previous_was_whitespace => return Some(index),
            _ => {}
        }
        saw_value |= !character.is_whitespace();
        previous_was_whitespace = character.is_whitespace();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn relative_includes_expand_in_place() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("zz/config");
        write(&main, "palette-show-keys = on\ninclude = hosts.conf\n");
        write(&root.path().join("zz/hosts.conf"), "host-box = ssh://box\n");

        let expanded = expand_config_file(&main).unwrap();

        assert_eq!(
            expanded.source,
            "palette-show-keys = on\nhost-box = ssh://box\n"
        );
        assert_eq!(
            expanded.files,
            [main.clone(), root.path().join("zz/hosts.conf")]
        );
    }

    #[test]
    fn optional_missing_include_is_listed_but_empty() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("config");
        write(&main, "include = -missing.conf\ntray = false\n");

        let expanded = expand_config_file(&main).unwrap();

        assert_eq!(expanded.source, "tray = false\n");
        assert_eq!(expanded.files, [main, root.path().join("missing.conf")]);
    }

    #[test]
    fn required_missing_include_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("config");
        write(&main, "include = missing.conf\n");

        let error = expand_config_file(&main).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn includes_recursively_and_detects_cycles() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("config");
        let nested = root.path().join("nested.conf");
        write(&main, "include = nested.conf\n");
        write(&nested, "include = config\n");

        let error = expand_config_file(&main).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(error.to_string().contains("cycle"));
    }

    #[test]
    fn declared_include_paths_list_targets_without_reading_them() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("config");
        write(&main, "include = -missing.conf\ntray = false\n");

        assert_eq!(
            declared_include_paths(&main),
            [main, root.path().join("missing.conf")]
        );
    }

    #[test]
    fn optional_does_not_hide_missing_nested_required_file() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("config");
        let nested = root.path().join("nested.conf");
        write(&main, "include = -nested.conf\n");
        write(&nested, "include = missing.conf\n");

        assert_eq!(
            expand_config_file(&main).unwrap_err().kind(),
            ErrorKind::NotFound
        );
        assert_eq!(declared_include_paths(&main).len(), 3);
    }

    #[test]
    fn expanded_output_has_a_combined_size_limit() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("config");
        let chunk = root.path().join("chunk.conf");
        write(&chunk, &format!("{}\n", "x".repeat(40_000)));
        write(&main, "include = chunk.conf\ninclude = chunk.conf\n");
        assert_eq!(
            expand_config_file(&main).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
    }

    #[test]
    fn quoted_and_commented_include_values_parse() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("config");
        write(&main, "include = \"has space.conf\" # trailing\n");
        write(&root.path().join("has space.conf"), "tray = false\n");

        let expanded = expand_config_file(&main).unwrap();
        assert_eq!(expanded.source, "tray = false\n");
    }
}
