#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quote {
    None,
    Single,
    Double,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub raw: String,
    pub value: String,
    pub quote: Quote,
    pub op: bool,
    pub space_before: bool,
}

pub fn tokenize(s: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut toks = Vec::new();
    let mut space_before = false;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            space_before = true;
            i += 1;
            continue;
        }
        if is_op(chars[i]) {
            let start = i;
            while i < chars.len() && is_op(chars[i]) {
                i += 1;
            }
            let raw: String = chars[start..i].iter().collect();
            toks.push(Token {
                value: raw.clone(),
                raw,
                quote: Quote::None,
                op: true,
                space_before,
            });
            space_before = false;
            continue;
        }
        let start = i;
        let mut value = String::new();
        let mut quote_segments = 0;
        let mut plain_segment = false;
        let mut last_quote = Quote::None;
        while i < chars.len() && !chars[i].is_whitespace() && !is_op(chars[i]) {
            match chars[i] {
                '\'' => {
                    quote_segments += 1;
                    last_quote = Quote::Single;
                    i += 1;
                    loop {
                        if i >= chars.len() {
                            return None;
                        }
                        match chars[i] {
                            '\'' => {
                                i += 1;
                                break;
                            }
                            '\\' if i + 1 < chars.len() && matches!(chars[i + 1], '\\' | '\'') => {
                                value.push(chars[i + 1]);
                                i += 2;
                            }
                            c => {
                                value.push(c);
                                i += 1;
                            }
                        }
                    }
                }
                '"' => {
                    quote_segments += 1;
                    last_quote = Quote::Double;
                    i += 1;
                    loop {
                        if i >= chars.len() {
                            return None;
                        }
                        match chars[i] {
                            '"' => {
                                i += 1;
                                break;
                            }
                            '\\' if i + 1 < chars.len()
                                && matches!(chars[i + 1], '"' | '\\' | '$') =>
                            {
                                value.push(chars[i + 1]);
                                i += 2;
                            }
                            c => {
                                value.push(c);
                                i += 1;
                            }
                        }
                    }
                }
                '\\' => {
                    if i + 1 >= chars.len() {
                        return None;
                    }
                    plain_segment = true;
                    value.push(chars[i + 1]);
                    i += 2;
                }
                c => {
                    plain_segment = true;
                    value.push(c);
                    i += 1;
                }
            }
        }
        let raw: String = chars[start..i].iter().collect();
        let quote = if quote_segments == 1 && !plain_segment {
            last_quote
        } else {
            Quote::None
        };
        toks.push(Token {
            raw,
            value,
            quote,
            op: false,
            space_before,
        });
        space_before = false;
    }
    if toks.is_empty() { None } else { Some(toks) }
}

fn is_op(c: char) -> bool {
    matches!(c, '|' | ';' | '&' | '<' | '>')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_words() {
        let t = tokenize("git push origin main").unwrap();
        assert_eq!(t.len(), 4);
        assert_eq!(t[3].raw, "main");
    }

    #[test]
    fn quoted_blob_is_one_token() {
        let t = tokenize("ssh prod-3 \"journalctl -u api --since '2 hours ago'\"").unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(t[2].quote, Quote::Double);
        assert_eq!(t[2].value, "journalctl -u api --since '2 hours ago'");
    }

    #[test]
    fn operators_split() {
        let t = tokenize("docker tag a && docker push a").unwrap();
        assert_eq!(t.len(), 7);
        assert!(t[3].op);
        assert_eq!(t[3].raw, "&&");
    }

    #[test]
    fn unterminated_quote_rejected() {
        assert!(tokenize("echo \"oops").is_none());
    }

    #[test]
    fn redirect_adjacency_tracked() {
        let t = tokenize("cmd 2>&1 | grep x").unwrap();
        assert_eq!(t[0].raw, "cmd");
        assert_eq!(t[1].raw, "2");
        assert!(t[1].space_before);
        assert_eq!(t[2].raw, ">&");
        assert!(!t[2].space_before);
        assert_eq!(t[3].raw, "1");
        assert!(!t[3].space_before);
    }
}
