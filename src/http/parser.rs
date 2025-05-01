use std::sync::Arc;

use percent_encoding::percent_decode_str;

#[derive(Clone, Debug)]
pub(super) struct Parser(Arc<regex::Regex>);

impl Default for Parser {
    fn default() -> Self {
        Self(Arc::new(
            regex::Regex::new(r#"<a(?:.*)? href="([^"]+)"(?:.*)?>"#).unwrap(),
        ))
    }
}

impl Parser {
    pub fn parse<'s, 'h>(&'s self, html: &'h str) -> impl Iterator<Item = String> + 'h
    where
        's: 'h,
    {
        self.0
            .captures_iter(html)
            .filter_map(|cap| cap.get(1))
            .filter(|cap| {
                let value = cap.as_str();
                !value.starts_with('?') && !value.starts_with("../") && value != "/"
            })
            .map(|cap| {
                percent_decode_str(cap.as_str())
                    .decode_utf8_lossy()
                    .to_string()
            })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn should_parse_apache() {
        let page = include_str!("../../assets/apache.html");
        let res = super::Parser::default().parse(page).collect::<Vec<_>>();
        assert_eq!(res.len(), 46, "{res:?}");
    }

    #[test]
    fn should_parse_nginx() {
        let page = include_str!("../../assets/nginx.html");
        let res = super::Parser::default().parse(page).collect::<Vec<_>>();
        assert_eq!(res.len(), 284, "{res:?}");
    }
}
