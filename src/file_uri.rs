//! File URI conversion for LSP source documents, without a general URL library.
//!
//! URI syntax, the file scheme, native paths, and percent encoding are handled
//! here. Empty authorities and `localhost` refer to local
//! paths; Windows additionally supports ASCII UNC authorities. Paths need not
//! exist, and conversion never performs hostname lookup or IDNA mapping.
//! See https://www.rfc-editor.org/rfc/rfc8089.html for file URI conventions.
use std::path::{Path, PathBuf};

/// Validate source identifiers while preserving their spelling as editor keys.
/// Dodo supports file documents and opaque `untitled:` editor buffers only.
pub(crate) fn validate(uri: &str) -> Result<(), &'static str> {
    if let Some(tail) = uri.strip_prefix("untitled:") {
        let (rest, fragment) = tail.split_once('#').unwrap_or((tail, ""));
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        validate_component(path, b"/")?;
        validate_component(query, b"/?")?;
        validate_component(fragment, b"/?")
    } else {
        to_path(uri).map(|_| ())
    }
}

// RFC 3986 pchar plus the delimiters allowed in the given component. URI text
// is ASCII; Unicode and native filename bytes must be percent encoded.
fn validate_component(text: &str, delimiters: &[u8]) -> Result<(), &'static str> {
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            for _ in 0..2 {
                if !bytes.next().is_some_and(|b| b.is_ascii_hexdigit()) {
                    return Err("invalid percent escape in source URI");
                }
            }
        } else if !(byte.is_ascii_alphanumeric()
            || b"-._~!$&'()*+,;=:@".contains(&byte)
            || delimiters.contains(&byte))
        {
            return Err("invalid character in source URI");
        }
    }
    Ok(())
}

pub(crate) fn to_path(uri: &str) -> Result<PathBuf, &'static str> {
    let (scheme, tail) = uri.split_once(':').ok_or("source URI must have a scheme")?;
    if !scheme.eq_ignore_ascii_case("file") {
        return Err("only file URIs are supported");
    }
    if tail.contains(['?', '#']) {
        return Err("source URI must not have a query or fragment");
    }
    let (authority, raw) = if let Some(tail) = tail.strip_prefix("//") {
        let end = tail.find('/').unwrap_or(tail.len());
        (&tail[..end], &tail[end..])
    } else {
        ("", tail)
    };
    let host = if authority.eq_ignore_ascii_case("localhost") {
        ""
    } else {
        authority
    };
    if !raw.starts_with('/') {
        return Err("source URI must have an absolute path");
    }
    validate_component(raw, b"/")?;
    // Resolve URI dot segments before converting to a native path, including
    // escaped dots. Never decode twice: `%2520` names a literal `%20`.
    let path = decode_path(raw, cfg!(windows))?;

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        if !host.is_empty() {
            return Err("only local file URIs are supported");
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(path)))
    }
    #[cfg(windows)]
    {
        windows_path(host, &path).map(PathBuf::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        if !host.is_empty() {
            return Err("only local file URIs are supported");
        }
        String::from_utf8(path)
            .map(PathBuf::from)
            .map_err(|_| "source path is not UTF-8")
    }
}

pub(crate) fn from_path(path: &Path) -> Result<String, &'static str> {
    if !path.is_absolute() {
        return Err("source path must be absolute");
    }
    #[cfg(unix)]
    let text = {
        use std::os::unix::ffi::OsStrExt;
        format!("file://{}", encode_path(path.as_os_str().as_bytes())?)
    };
    #[cfg(windows)]
    let text = {
        use std::path::{Component, Prefix};
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return Err("source path must have a drive or UNC prefix");
        };
        let root = match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                format!("file:///{}:", char::from(drive))
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                let server = server.to_str().ok_or("UNC host is not UTF-8")?;
                validate_unc_host(server)?;
                let share = share.to_str().ok_or("UNC share is not UTF-8")?;
                format!("file://{server}/{}", encode_path(share.as_bytes())?)
            }
            _ => return Err("unsupported source path prefix"),
        };
        // Slice the spelling, not Path components: a UNC prefix has an
        // implicit root component that Path::strip_prefix would also remove.
        let spelling = path.to_str().ok_or("source path is not UTF-8")?;
        let prefix = prefix
            .as_os_str()
            .to_str()
            .ok_or("source prefix is not UTF-8")?;
        let tail = spelling
            .strip_prefix(prefix)
            .ok_or("invalid source path prefix")?;
        let tail = if tail.is_empty() {
            "/".into()
        } else {
            tail.replace('\\', "/")
        };
        format!("{root}{}", encode_path(tail.as_bytes())?)
    };
    #[cfg(not(any(unix, windows)))]
    let text = format!(
        "file://{}",
        encode_path(path.to_str().ok_or("source path is not UTF-8")?.as_bytes())?
    );
    Ok(text)
}

fn encode_path(bytes: &[u8]) -> Result<String, &'static str> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for &byte in bytes {
        if byte == 0 {
            return Err("source path must not contain NUL");
        }
        if byte.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    Ok(encoded)
}

fn decode_segment(segment: &str) -> Result<Vec<u8>, &'static str> {
    let mut decoded = Vec::with_capacity(segment.len());
    let mut bytes = segment.bytes();
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            let mut hex = || bytes.next().and_then(|b| char::from(b).to_digit(16));
            let high = hex().ok_or("invalid percent escape in source URI")?;
            let low = hex().ok_or("invalid percent escape in source URI")?;
            (high * 16 + low) as u8
        } else {
            byte
        };
        if byte == 0 {
            return Err("source path must not contain NUL");
        }
        decoded.push(byte);
    }
    Ok(decoded)
}

fn decode_path(raw: &str, windows: bool) -> Result<Vec<u8>, &'static str> {
    let mut segments: Vec<Vec<u8>> = Vec::new();
    let mut parts = raw[1..].split('/').peekable();
    while let Some(part) = parts.next() {
        let segment = decode_segment(part)?;
        match segment.as_slice() {
            b"." => (),
            b".." => {
                // Do not walk above a Windows drive or UNC share root.
                if segments.len() > usize::from(windows) {
                    segments.pop();
                }
            }
            _ => {
                segments.push(segment);
                continue;
            }
        }
        if parts.peek().is_none() {
            segments.push(Vec::new());
        }
    }
    let mut decoded = vec![b'/'];
    decoded.extend(segments.join(&b'/'));
    Ok(decoded)
}

#[cfg(any(windows, test))]
fn validate_unc_host(host: &str) -> Result<(), &'static str> {
    if host.is_empty()
        || host == "."
        || host == ".."
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
    {
        return Err("unsupported UNC host in source URI");
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn windows_path(host: &str, path: &[u8]) -> Result<String, &'static str> {
    let path = std::str::from_utf8(path).map_err(|_| "source path is not UTF-8")?;
    let tail = path
        .strip_prefix('/')
        .ok_or("source URI must have an absolute path")?;
    if !host.is_empty() {
        validate_unc_host(host)?;
        if tail.is_empty() || tail.starts_with(['/', '\\']) {
            return Err("UNC source URI must have a share");
        }
        return Ok(format!("\\\\{host}\\{}", tail.replace('/', "\\")));
    }
    let bytes = tail.as_bytes();
    if bytes.len() < 2
        || !bytes[0].is_ascii_alphabetic()
        || !matches!(bytes[1], b':' | b'|')
        || (bytes.len() > 2 && !matches!(bytes[2], b'/' | b'\\'))
    {
        return Err("source URI must have an absolute Windows drive path");
    }
    let rest = tail[2..].replace('/', "\\");
    Ok(format!(
        "{}:{}",
        char::from(bytes[0]),
        if rest.is_empty() { "\\" } else { &rest }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<PathBuf, String> {
        to_path(text).map_err(str::to_owned)
    }

    #[test]
    fn rejects_invalid_or_unsupported_uris() {
        for uri in [
            "",
            "/tmp/a",
            "https://localhost/tmp/a",
            "untitled:main",
            "file:relative",
            "file:",
            "file://localhost",
            "file:///tmp/a?",
            "file:///tmp/a#",
            "file:///tmp/a?query",
            "file:///tmp/a#fragment",
            "file:///tmp/%",
            "file:///tmp/%0",
            "file:///tmp/%GG",
            "file:///tmp/%0g",
            "file:///tmp/%g0",
            "file:///tmp/%00",
            "file:///tmp/a b",
            "file:///tmp/é",
            "file:///tmp/a\n",
            "file://user@localhost/tmp/a",
            "file://localhost:80/tmp/a",
        ] {
            assert!(parse(uri).is_err(), "accepted {uri:?}");
        }
    }

    #[test]
    fn rejects_relative_paths() {
        for path in ["", "a.dodo", "./a.dodo", "../a.dodo"] {
            assert!(from_path(Path::new(path)).is_err(), "accepted {path:?}");
        }
    }

    #[test]
    fn untitled_identifiers_validate_uri_syntax_without_normalizing_spelling() {
        for uri in [
            "untitled:",
            "untitled:Untitled-1",
            "untitled:/folder/%C3%A9%20%F0%9F%98%80.dodo",
            "untitled:a%2fb?query=/a?b#fragment?x/y",
            "untitled:!$&'()*+,;=:@-._~",
        ] {
            assert!(validate(uri).is_ok(), "rejected {uri}");
        }
        for tail in [
            "a b", "é", "😀", "a\n", "a\r", "a\t", "a\0", "%", "%0", "%g0", "%0g", "a\\b", "[a]",
            "a|b", "<a>", "a^b", "a`b", "{a}", "a\u{7f}", "a#b#c", "a?%xy", "a#%xy",
        ] {
            assert!(
                validate(&format!("untitled:{tail}")).is_err(),
                "accepted {tail:?}"
            );
        }
    }

    #[test]
    fn file_paths_reject_raw_characters_previously_validated_by_the_uri_library() {
        // Run on every platform: each URI includes a drive for Windows.
        for byte in 0..=255_u8 {
            let valid = byte.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@".contains(&byte);
            let uri = format!("file:///C:/a{}z", char::from(byte));
            assert_eq!(to_path(&uri).is_ok(), valid, "byte {byte}");
        }
    }

    #[test]
    fn percent_encoding_has_known_results() {
        let path = b"/a z/%20+#?\\\"<>[]^`{|}\x7f\x01";
        assert_eq!(
            encode_path(path).unwrap(),
            "/a%20z/%2520+%23%3F%5C%22%3C%3E%5B%5D%5E%60%7B%7C%7D%7F%01"
        );
        assert_eq!(decode_segment("%c3%a9+%252F").unwrap(), "é+%2F".as_bytes());
        for invalid in ["%", "%0", "%zz", "%0z", "%z0", "%00"] {
            assert!(decode_segment(invalid).is_err(), "accepted {invalid:?}");
        }
        assert!(encode_path(b"/a\0b").is_err());
    }

    #[test]
    fn dot_segments_are_resolved_without_decoding_twice() {
        for (uri, expected) in [
            ("/a/./b/../c", "/a/c"),
            ("/a/%2e/%2E%2e/c", "/c"),
            ("/../../a", "/a"),
            ("/a/.", "/a/"),
            ("/a/b/..", "/a/"),
            ("/a//b/", "/a//b/"),
            ("/a/%252E%252E/b", "/a/%2E%2E/b"),
            ("/a%2Fb/c", "/a/b/c"),
        ] {
            assert_eq!(
                decode_path(uri, false).unwrap(),
                expected.as_bytes(),
                "{uri}"
            );
        }
        assert_eq!(decode_path("/C:/../../a", true).unwrap(), b"/C:/a");
        assert_eq!(decode_path("/share/../../a", true).unwrap(), b"/share/a");
    }

    #[cfg(unix)]
    #[test]
    fn unix_paths_and_local_authorities() {
        for (path, uri) in [
            ("/", "file:///"),
            ("/tmp/a.dodo", "file:///tmp/a.dodo"),
            (
                "/tmp/é 😀 #?%.dodo",
                "file:///tmp/%C3%A9%20%F0%9F%98%80%20%23%3F%25.dodo",
            ),
            ("/tmp/a+b%20\\c", "file:///tmp/a+b%2520%5Cc"),
            ("/tmp/!$&'()*+,;=:@-._~", "file:///tmp/!$&'()*+,;=:@-._~"),
        ] {
            assert_eq!(from_path(Path::new(path)).unwrap().as_str(), uri);
            assert_eq!(parse(uri).unwrap(), Path::new(path));
        }
        for uri in [
            "file:/tmp/a",
            "file:///tmp/a",
            "FILE://LOCALHOST/tmp/a",
            "file://localhost/tmp/a",
        ] {
            assert_eq!(parse(uri).unwrap(), Path::new("/tmp/a"));
        }
        for uri in [
            "file://remote/tmp/a",
            "file://127.0.0.1/tmp/a",
            "file://[::1]/tmp/a",
        ] {
            assert!(parse(uri).is_err(), "accepted {uri}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_round_trips_every_non_nul_filename_byte() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        for byte in 1..=255 {
            if byte == b'/' {
                continue;
            }
            let path = PathBuf::from(OsString::from_vec(vec![b'/', b'a', byte, b'z']));
            let uri = from_path(&path).unwrap();
            assert!(uri.as_str().is_ascii());
            assert_eq!(to_path(&uri).unwrap(), path, "byte {byte}");
        }
        assert_eq!(
            parse("file:///a%FFz").unwrap(),
            PathBuf::from(OsString::from_vec(b"/a\xffz".to_vec()))
        );
        assert!(from_path(Path::new(&OsString::from_vec(b"/a\0b".to_vec()))).is_err());
    }

    #[test]
    fn windows_drive_and_unc_decoding() {
        for (host, path, expected) in [
            ("", "/C:/Users/a%20b/%C3%A9.dodo", "C:\\Users\\a b\\é.dodo"),
            ("", "/c%3A/a", "c:\\a"),
            ("", "/D%7C/a", "D:\\a"),
            ("", "/C:", "C:\\"),
            ("server", "/share/a%20b.dodo", "\\\\server\\share\\a b.dodo"),
        ] {
            assert_eq!(
                windows_path(host, &decode_path(path, true).unwrap()).unwrap(),
                expected
            );
        }
        for (host, path) in [
            ("", "/tmp/a"),
            ("", "/C:relative"),
            ("", "/1:/a"),
            ("", "/C:/%FF"),
            ("server", "/"),
            ("server", "//a"),
            ("user@server", "/share/a"),
            ("server:80", "/share/a"),
            (".", "/share/a"),
        ] {
            assert!(
                windows_path(host, &decode_path(path, true).unwrap()).is_err(),
                "{host} {path}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn native_windows_paths_encode_drives_unc_and_verbatim_prefixes() {
        for (path, uri, decoded) in [
            (
                r"\\server\share",
                "file://server/share/",
                r"\\server\share\",
            ),
            (r"C:\", "file:///C:/", r"C:\"),
            (
                r"C:\a b\é.dodo",
                "file:///C:/a%20b/%C3%A9.dodo",
                r"C:\a b\é.dodo",
            ),
            (r"\\?\C:\a b.dodo", "file:///C:/a%20b.dodo", r"C:\a b.dodo"),
            (
                r"\\server\share\a.dodo",
                "file://server/share/a.dodo",
                r"\\server\share\a.dodo",
            ),
            (
                r"\\?\UNC\server\share\a.dodo",
                "file://server/share/a.dodo",
                r"\\server\share\a.dodo",
            ),
        ] {
            assert_eq!(from_path(Path::new(path)).unwrap().as_str(), uri);
            assert_eq!(parse(uri).unwrap(), Path::new(decoded));
        }
        assert_eq!(parse("FILE://LOCALHOST/C:/a").unwrap(), Path::new(r"C:\a"));
        for path in [r"C:relative", r"\rooted", r"\\.\COM1"] {
            assert!(from_path(Path::new(path)).is_err());
        }
    }
}
