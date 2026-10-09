//! Minimal parser for Valve's KeyValues text format (.vdf / .acf files).

#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Obj(Object),
}

#[derive(Debug, Clone, Default)]
pub struct Object(Vec<(String, Value)>);

impl Object {
    /// Looks up a key, ignoring case (Valve isn't consistent about it).
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            Value::Obj(_) => None,
        }
    }

    pub fn get_obj(&self, key: &str) -> Option<&Object> {
        match self.get(key)? {
            Value::Obj(o) => Some(o),
            Value::Str(_) => None,
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '"' => {
                let mut s = String::new();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => return Err("unterminated escape".into()),
                        },
                        Some(other) => s.push(other),
                        None => return Err("unterminated string".into()),
                    }
                }
                tokens.push(Token::Str(s));
            }
            '/' if chars.peek() == Some(&'/') => {
                // Line comment.
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            _ => {
                // Unquoted token: read until whitespace or brace.
                let mut s = String::from(c);
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || next == '{' || next == '}' || next == '"' {
                        break;
                    }
                    s.push(next);
                    chars.next();
                }
                tokens.push(Token::Str(s));
            }
        }
    }
    Ok(tokens)
}

fn parse_object<I: Iterator<Item = Token>>(
    tokens: &mut std::iter::Peekable<I>,
    nested: bool,
) -> Result<Object, String> {
    let mut entries = Vec::new();
    loop {
        match tokens.next() {
            None if nested => return Err("unexpected end of input".into()),
            None => return Ok(Object(entries)),
            Some(Token::Close) if nested => return Ok(Object(entries)),
            Some(Token::Close) => return Err("unexpected '}'".into()),
            Some(Token::Open) => return Err("unexpected '{'".into()),
            Some(Token::Str(key)) => match tokens.next() {
                Some(Token::Str(value)) => entries.push((key, Value::Str(value))),
                Some(Token::Open) => entries.push((key, Value::Obj(parse_object(tokens, true)?))),
                _ => return Err(format!("missing value for key '{key}'")),
            },
        }
    }
}

/// Parses a whole .vdf/.acf document into its root object.
pub fn parse(input: &str) -> Result<Object, String> {
    let tokens = tokenize(input)?;
    parse_object(&mut tokens.into_iter().peekable(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_objects_and_escapes() {
        let doc = r#"
            "libraryfolders"
            {
                "0"
                {
                    "path"  "C:\\Program Files (x86)\\Steam"
                    "apps" { "730" "123" }
                }
            }
        "#;
        let root = parse(doc).unwrap();
        let lib = root
            .get_obj("libraryfolders")
            .unwrap()
            .get_obj("0")
            .unwrap();
        assert_eq!(lib.get_str("path"), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(lib.get_obj("apps").unwrap().get_str("730"), Some("123"));
    }

    #[test]
    fn keys_are_case_insensitive() {
        let root = parse(r#""AppState" { "LastPlayed" "5" }"#).unwrap();
        assert_eq!(
            root.get_obj("appstate").unwrap().get_str("lastplayed"),
            Some("5")
        );
    }

    #[test]
    fn rejects_unterminated_input() {
        assert!(parse(r#""a" { "b" "c""#).is_err());
        assert!(parse(r#""a" "b"#).is_err());
    }
}
