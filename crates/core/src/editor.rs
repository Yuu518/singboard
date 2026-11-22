use jsonc_parser::{
    CollectOptions, CommentCollectionStrategy, ast::Value, common::Ranged, parse_to_ast,
    tokens::Token,
};
use std::ops::Range;
pub fn modules(source: &str) -> Result<Vec<(String, Range<usize>)>, String> {
    let parsed = parse_to_ast(source, &Default::default(), &Default::default())
        .map_err(|e| e.to_string())?;
    let Some(Value::Object(root)) = parsed.value else {
        return Err("配置的最外层必须是对象".into());
    };
    Ok(root
        .properties
        .into_iter()
        .map(|p| (p.name.into_string(), p.value.start()..p.value.end()))
        .collect())
}
pub fn format(source: &str) -> Result<String, String> {
    let options = CollectOptions {
        comments: CommentCollectionStrategy::AsTokens,
        tokens: true,
    };
    let parsed = parse_to_ast(source, &options, &Default::default()).map_err(|e| e.to_string())?;
    if parsed.value.is_none() {
        return Err("没有可格式化的内容".into());
    }
    let mut out = String::new();
    let mut depth = 0usize;
    let mut newline = false;
    let mut last_open = false;
    for item in parsed.tokens.unwrap_or_default() {
        let closing = matches!(item.token, Token::CloseBrace | Token::CloseBracket);
        if closing {
            depth = depth.saturating_sub(1);
            newline = !last_open;
        }
        if newline {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&"  ".repeat(depth));
        } else if matches!(item.token, Token::CommentLine(_) | Token::CommentBlock(_))
            && !out.is_empty()
            && !out.ends_with([' ', '\n', '\t'])
        {
            out.push(' ');
        }
        out.push_str(item.text(source));
        last_open = matches!(item.token, Token::OpenBrace | Token::OpenBracket);
        newline = last_open
            || matches!(
                item.token,
                Token::Comma | Token::CommentLine(_) | Token::CommentBlock(_)
            );
        if last_open {
            depth += 1;
        }
        if matches!(item.token, Token::Colon) {
            out.push(' ');
        }
    }
    if source.ends_with('\n') {
        out.push('\n');
    }
    parse_to_ast(&out, &Default::default(), &Default::default()).map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn toggle_comment(source: &str, range: Range<usize>) -> (String, Range<usize>) {
    let start_pos = range.start.min(source.len());
    let end_pos = range.end.min(source.len()).max(start_pos);
    let line_start = source[..start_pos].rfind('\n').map_or(0, |i| i + 1);
    let line_end = if end_pos > start_pos && source.as_bytes().get(end_pos - 1) == Some(&b'\n') {
        end_pos - 1
    } else {
        source[end_pos..]
            .find('\n')
            .map_or(source.len(), |i| end_pos + i)
    };
    let slice = &source[line_start..line_end];
    let has_non_empty = slice.split('\n').any(|v| !v.trim().is_empty());
    let remove = has_non_empty
        && slice
            .split('\n')
            .filter(|v| !v.trim().is_empty())
            .all(|v| v.trim_start().starts_with("//"));
    let changed = slice
        .split('\n')
        .map(|v| {
            let (line, cr) = v.strip_suffix('\r').map_or((v, ""), |s| (s, "\r"));
            if remove {
                let trimmed = line.trim_start();
                let indent = &line[..line.len() - trimmed.len()];
                let rest = trimmed
                    .strip_prefix("// ")
                    .or_else(|| trimmed.strip_prefix("//"))
                    .unwrap_or(trimmed);
                format!("{indent}{rest}{cr}")
            } else if line.trim().is_empty() && has_non_empty {
                v.to_string()
            } else {
                let trimmed = line.trim_start();
                let indent = &line[..line.len() - trimmed.len()];
                format!("{indent}// {trimmed}{cr}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = source.to_string();
    out.replace_range(line_start..line_end, &changed);
    (out, line_start..line_start + changed.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn module_splices_and_format_preserve_comments_and_literals() {
        let s = r#"{"名前":"a//b",/* keep */"x":[2,// line
3,]}"#;
        let parts = modules(s).unwrap();
        assert_eq!(&s[parts[0].1.clone()], r#""a//b""#);
        let out = format(s).unwrap();
        for literal in [r#""a//b""#, "/* keep */", "// line"] {
            assert!(out.contains(literal));
        }
        assert_eq!(format(&out).unwrap(), out);
    }

    #[test]
    fn toggle_comment_handles_multiline_indentation_and_roundtrip() {
        let src = "{\n  \"a\": 1,\n  \"b\": 2\n}\n";
        let (commented, sel) = toggle_comment(src, 4..18);
        assert_eq!(commented, "{\n  // \"a\": 1,\n  // \"b\": 2\n}\n");
        let (restored, _) = toggle_comment(&commented, sel);
        assert_eq!(restored, src);
    }
}
