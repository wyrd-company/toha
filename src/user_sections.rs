// ---
// relationships:
//   implements: template-format
// ---
//! User-owned bodies inside otherwise generated text files.
use std::{collections::BTreeMap, ops::Range};

#[derive(Debug)]
struct Section {
    body: Range<usize>,
}

fn sections(text: &str) -> Result<BTreeMap<String, Section>, String> {
    let mut found = BTreeMap::new();
    let mut open: Option<(String, String, String, usize)> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let marker = content
            .find(">>> toha:user ")
            .map(|at| (at, true))
            .or_else(|| content.find("<<< toha:user ").map(|at| (at, false)));
        if let Some((at, begin)) = marker {
            let tag = if begin {
                ">>> toha:user "
            } else {
                "<<< toha:user "
            };
            let rest = &content[at + tag.len()..];
            let delimiter = if begin { " >>>" } else { " <<<" };
            let (key, suffix) = rest
                .split_once(delimiter)
                .ok_or_else(|| "malformed user-section marker".to_owned())?;
            crate::inject::RegionKey::parse(key)?;
            let prefix = &content[..at];
            if begin {
                if open.is_some() || found.contains_key(key) {
                    return Err(format!("nested or duplicate user section `{key}`"));
                }
                open = Some((
                    key.into(),
                    prefix.into(),
                    suffix.into(),
                    offset + line.len(),
                ));
            } else {
                let Some((name, before, after, start)) = open.take() else {
                    return Err(format!("unpaired user section `{key}`"));
                };
                if name != key || before != prefix || after != suffix {
                    return Err(format!("mismatched user-section markers for `{key}`"));
                }
                found.insert(
                    name,
                    Section {
                        body: start..offset,
                    },
                );
            }
        }
        offset += line.len();
    }
    if let Some((key, ..)) = open {
        return Err(format!("unpaired user section `{key}`"));
    }
    Ok(found)
}

pub(crate) fn validate(text: &str) -> Result<(), String> {
    sections(text).map(|_| ())
}

/// Returns a folded image only when either image declares user sections.
/// Marker lines and everything outside them come from the template.
pub(crate) fn preserve(desired: &str, current: Option<&[u8]>) -> Result<Option<Vec<u8>>, String> {
    let wanted = sections(desired)?;
    let current = match current {
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) if wanted.is_empty() => return Ok(None),
            Err(_) => return Err("user-section target is not UTF-8".into()),
        },
        None => "",
    };
    let existing = sections(current)?;
    if wanted.is_empty() && existing.is_empty() {
        return Ok(None);
    }
    for key in existing.keys() {
        if !wanted.contains_key(key) {
            return Err(format!(
                "user section `{key}` is absent from the template; move its content and remove its markers before applying"
            ));
        }
    }
    let mut replacements: Vec<_> = wanted
        .iter()
        .filter_map(|(key, section)| {
            existing
                .get(key)
                .map(|old| (section.body.clone(), &current[old.body.clone()]))
        })
        .collect();
    replacements.sort_by_key(|(range, _)| range.start);
    let mut result = String::new();
    let mut offset = 0;
    for (range, body) in replacements {
        result.push_str(&desired[offset..range.start]);
        result.push_str(body);
        offset = range.end;
    }
    result.push_str(&desired[offset..]);
    Ok(Some(result.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_multiple_reordered_sections_and_exact_body_bytes() {
        let old = "// >>> toha:user a >>>\r\ncustom α\r\n// <<< toha:user a <<<\r\n// >>> toha:user b >>>\n\n// <<< toha:user b <<<\n";
        let new = "new\n// >>> toha:user b >>>\ndefault\n// <<< toha:user b <<<\n// >>> toha:user a >>>\ndefault\n// <<< toha:user a <<<\ntail\n";
        let result = preserve(new, Some(old.as_bytes())).unwrap().unwrap();
        assert_eq!(
            String::from_utf8(result).unwrap(),
            "new\n// >>> toha:user b >>>\n\n// <<< toha:user b <<<\n// >>> toha:user a >>>\ncustom α\r\n// <<< toha:user a <<<\ntail\n"
        );
    }

    #[test]
    fn rejects_nested_duplicate_mismatched_and_unpaired_markers() {
        for text in [
            ">>> toha:user a >>>\n>>> toha:user b >>>\n",
            ">>> toha:user a >>>\n<<< toha:user b <<<\n",
            ">>> toha:user a >>>\n",
            "<<< toha:user a <<<\n",
            ">>> toha:user a >>>\n<<< toha:user a <<<\n>>> toha:user a >>>\n<<< toha:user a <<<\n",
            "# >>> toha:user a >>>\n// <<< toha:user a <<<\n",
            ">>> toha:user Invalid >>>\n<<< toha:user Invalid <<<\n",
        ] {
            assert!(validate(text).is_err(), "{text}");
        }
    }

    #[test]
    fn refuses_to_discard_a_removed_section() {
        let old = ">>> toha:user a >>>\nprecious\n<<< toha:user a <<<\n";
        assert!(
            preserve("new\n", Some(old.as_bytes()))
                .unwrap_err()
                .contains("absent from the template")
        );
    }

    #[test]
    fn new_sections_use_template_defaults_and_binary_targets_are_checked() {
        let desired =
            "<!-- >>> toha:user notes >>> -->\ndefault\n<!-- <<< toha:user notes <<< -->\n";
        assert_eq!(
            preserve(desired, None).unwrap().unwrap(),
            desired.as_bytes()
        );
        assert!(preserve(desired, Some(&[0xff])).is_err());
        assert!(preserve("ordinary\n", Some(&[0xff])).unwrap().is_none());
    }
}
