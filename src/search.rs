//! Everything but the rofi glue: the engines file, which rows to show for the
//! input and what Enter does.

#[derive(Debug, Clone, PartialEq)]
pub struct Engine {
    pub keyword: String,
    pub name: String,
    /// With `{}` for the query.
    pub url: String,
    /// Image path or icon theme name, instead of the site's favicon.
    pub icon: Option<String>,
    /// In the engines file.
    position: usize,
}

#[derive(Default)]
struct Section {
    keyword: String,
    name: Option<String>,
    url: Option<String>,
    icon: Option<String>,
}

/// Parses the engines file, an INI file with one section per engine named by
/// its keyword:
///
/// ```ini
/// [im]
/// name = IMDb
/// url  = https://www.imdb.com/find/?q={}
/// icon = ~/Pictures/imdb.png   # optional
/// ```
///
/// Lines starting with `#` or `;` are comments. Problems are returned as
/// warnings next to the engines that could be read.
pub fn parse_engines(text: &str) -> (Vec<Engine>, Vec<String>) {
    let mut sections: Vec<Section> = Vec::new();
    let mut warnings = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(keyword) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push(Section { keyword: keyword.trim().to_lowercase(), ..Section::default() });
            continue;
        }
        let (Some((key, value)), Some(section)) = (line.split_once('='), sections.last_mut())
        else {
            warnings.push(format!("line {}: expected [keyword] or key = value", number + 1));
            continue;
        };
        let value = Some(value.trim().to_string());
        match key.trim() {
            "name" => section.name = value,
            "url" => section.url = value,
            "icon" => section.icon = value,
            other => warnings.push(format!("line {}: unknown key {other}", number + 1)),
        }
    }

    let mut engines = Vec::new();
    for section in sections {
        match section {
            Section { keyword, name: Some(name), url: Some(url), icon } if !keyword.is_empty() => {
                engines.push(Engine { keyword, name, url, icon, position: engines.len() });
            }
            Section { keyword, .. } => warnings.push(format!("[{keyword}] needs name and url")),
        }
    }
    (engines, warnings)
}

/// The host of an engine's URL, e.g. `www.imdb.com`.
pub fn host(url: &str) -> &str {
    let start = url.split_once("://").map_or(url, |(_, rest)| rest);
    start.split(['/', '?', '#']).next().unwrap_or(start)
}

/// The site shown next to an engine's name, e.g. `imdb.com`.
fn site(url: &str) -> &str {
    let host = host(url);
    host.strip_prefix("www.").unwrap_or(host)
}

/// Percent-encodes everything but unreserved characters, so spaces are `%20`.
fn encode(query: &str) -> String {
    let mut encoded = String::with_capacity(query.len());
    for byte in query.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// `example.com/x`, `localhost.dev:8080` or anything starting with `http(s)://`.
fn is_address(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("http://").or(lower.strip_prefix("https://")) {
        return !rest.is_empty() && !rest.contains(char::is_whitespace);
    }
    if text.contains(char::is_whitespace) {
        return false;
    }
    let authority = text.split('/').next().unwrap_or(text);
    let (host, port) = authority.split_once(':').unwrap_or((authority, ""));
    let labels: Vec<&str> = host.split('.').collect();
    let tld = labels[labels.len() - 1];
    labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'))
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
        && (!authority.contains(':') || (!port.is_empty() && port.chars().all(|c| c.is_ascii_digit())))
}

/// Splits the input into its first word (lowercased) and the rest after the
/// first space.
fn split_input(input: &str) -> (String, Option<&str>) {
    match input.split_once(' ') {
        Some((word, rest)) => (word.to_lowercase(), Some(rest)),
        None => (input.to_lowercase(), None),
    }
}

/// What Enter does.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Open(String),
    /// Replace the input with this and show the list again.
    Fill(String),
    Nothing,
}

pub struct Search {
    /// The default engine first.
    engines: Vec<Engine>,
    /// Per engine, for the current input.
    visible: Vec<bool>,
    /// What Enter on a row searches, `None` to pick the engine.
    query: Option<String>,
}

impl Search {
    pub fn new(engines: Vec<Engine>) -> Self {
        let visible = vec![true; engines.len()];
        Self { engines, visible, query: None }
    }

    pub fn len(&self) -> usize {
        self.engines.len()
    }

    pub fn engines(&self) -> &[Engine] {
        &self.engines
    }

    pub fn is_visible(&self, index: usize) -> bool {
        self.visible[index]
    }

    fn find_keyword(&self, word: &str) -> Option<usize> {
        self.engines.iter().position(|e| e.keyword == word)
    }

    /// Makes an engine the default, which puts it first, the others stay in
    /// the file's order. Returns false for an unknown keyword.
    pub fn set_default(&mut self, keyword: &str) -> bool {
        if self.find_keyword(keyword).is_none() {
            return false;
        }
        self.engines.sort_by_key(|e| (e.keyword != keyword, e.position));
        true
    }

    /// Makes the engine on that row the default and returns its keyword.
    pub fn set_default_index(&mut self, index: usize) -> Option<&str> {
        let keyword = self.engines.get(index)?.keyword.clone();
        self.set_default(&keyword);
        Some(&self.engines[0].keyword)
    }

    /// Works out the rows to show: "kw query" shows only that engine, a single
    /// word shows the engines it is part of (keyword, name or site), anything
    /// else shows the default engine.
    pub fn update(&mut self, input: &str) {
        let (word, rest) = split_input(input);
        let keyword_engine = self.find_keyword(&word);
        self.query = None;

        for (i, e) in self.engines.iter().enumerate() {
            self.visible[i] = match rest {
                Some(_) => keyword_engine == Some(i),
                None => {
                    e.keyword.contains(&word)
                        || e.name.to_lowercase().contains(&word)
                        || site(&e.url).contains(&word)
                }
            };
        }

        if let (Some(_), Some(rest)) = (keyword_engine, rest) {
            let query = rest.trim();
            if !query.is_empty() {
                self.query = Some(query.to_string());
            }
        } else if !self.visible.contains(&true) && !input.is_empty() && !self.engines.is_empty() {
            self.visible[0] = true;
            self.query = Some(input.trim().to_string());
        }
    }

    /// The row as Pango markup: keyword, name, and the query being searched
    /// or else the site.
    pub fn row(&self, index: usize) -> String {
        let e = &self.engines[index];
        let width = self.engines.iter().map(|e| e.keyword.chars().count()).max().unwrap_or(0);
        let mut detail = match &self.query {
            Some(query) => format!("“{query}”"),
            None => site(&e.url).to_string(),
        };
        if index == 0 {
            detail.push_str("  · default");
        }
        format!(
            "<tt>{}  </tt><b>{}</b><span alpha=\"60%\">   {}</span>",
            escape(&format!("{:<width$}", e.keyword)),
            escape(&e.name),
            escape(&detail),
        )
    }

    fn search_url(&self, index: usize, query: &str) -> String {
        self.engines[index].url.replace("{}", &encode(query))
    }

    /// What Enter does for the input. `selected` is the highlighted row, and
    /// `literal` is set for Ctrl+Enter or when no row is shown, which takes
    /// the text as typed.
    pub fn accept(&self, input: &str, selected: Option<usize>, literal: bool) -> Outcome {
        let text = input.trim();
        let (word, rest) = split_input(text);
        let rest = rest.map(str::trim).filter(|r| !r.is_empty());

        if is_address(text) {
            return Outcome::Open(if text.contains("://") {
                text.to_string()
            } else {
                format!("https://{text}")
            });
        }
        if literal || selected.is_none() {
            return match (self.find_keyword(&word), rest) {
                (Some(i), Some(rest)) => Outcome::Open(self.search_url(i, rest)),
                _ if !text.is_empty() && !self.engines.is_empty() => {
                    Outcome::Open(self.search_url(0, text))
                }
                _ => Outcome::Nothing,
            };
        }
        let Some(selected) = selected.filter(|&s| s < self.engines.len()) else {
            return Outcome::Nothing;
        };
        match &self.query {
            Some(query) => Outcome::Open(self.search_url(selected, query)),
            // An engine picked without a query: type it after its keyword.
            None => Outcome::Fill(format!("{} ", self.engines[selected].keyword)),
        }
    }
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGINES: &str = "\
# comment
[g]
name = Google
url  = https://www.google.com/search?q={}

[IM]
name = IMDb
url  = https://www.imdb.com/find/?q={}

; another comment
[gh]
name = GitHub
url  = https://github.com/search?q={}&type=repositories

[ok]
name =  OpenStreetMap Kraków\t
url = https://www.openstreetmap.org/search?query=kraków+{}#map
icon = ~/icons/osm.png
";

    fn search() -> Search {
        let (engines, warnings) = parse_engines(ENGINES);
        assert_eq!(warnings, Vec::<String>::new());
        Search::new(engines)
    }

    fn shown(s: &Search) -> Vec<String> {
        (0..s.len()).filter(|&i| s.is_visible(i)).map(|i| s.engines()[i].keyword.clone()).collect()
    }

    /// Types the input and presses Enter on the first row shown.
    fn enter(s: &mut Search, input: &str) -> Outcome {
        s.update(input);
        let selected = (0..s.len()).find(|&i| s.is_visible(i));
        s.accept(input, selected, false)
    }

    #[test]
    fn parses_engines() {
        let s = search();
        let keywords: Vec<_> = s.engines.iter().map(|e| e.keyword.as_str()).collect();
        assert_eq!(keywords, ["g", "im", "gh", "ok"]);
        assert_eq!(s.engines[3].name, "OpenStreetMap Kraków");
        assert_eq!(s.engines[3].url, "https://www.openstreetmap.org/search?query=kraków+{}#map");
        assert_eq!(s.engines[3].icon.as_deref(), Some("~/icons/osm.png"));
        assert_eq!(s.engines[0].icon, None);
        assert_eq!(host(&s.engines[3].url), "www.openstreetmap.org");
    }

    #[test]
    fn example_engines_parse() {
        let (engines, warnings) = parse_engines(include_str!("../engines.ini"));
        assert_eq!(warnings, Vec::<String>::new());
        assert!(!engines.is_empty());
    }

    #[test]
    fn reports_broken_engines() {
        let (engines, warnings) =
            parse_engines("name = orphan\n[a]\nname = A\n[b]\nurl = x\ncolor = red\n");
        assert!(engines.is_empty());
        assert_eq!(
            warnings,
            [
                "line 1: expected [keyword] or key = value",
                "line 6: unknown key color",
                "[a] needs name and url",
                "[b] needs name and url",
            ]
        );
    }

    #[test]
    fn filters_rows() {
        let mut s = search();
        s.update("im");
        assert_eq!(shown(&s), ["im"]);
        s.update("goo");
        assert_eq!(shown(&s), ["g"]);
        s.update("org");
        assert_eq!(shown(&s), ["ok"]);
        s.update("hub");
        assert_eq!(shown(&s), ["gh"]);
        s.update("IM the matrix");
        assert_eq!((shown(&s), s.query.as_deref()), (vec!["im".into()], Some("the matrix")));
        s.update("im ");
        assert_eq!((shown(&s), s.query.as_deref()), (vec!["im".into()], None));
        s.update("how to cook rice");
        assert_eq!((shown(&s), s.query.as_deref()), (vec!["g".into()], Some("how to cook rice")));
    }

    #[test]
    fn enter_searches_or_fills() {
        let mut s = search();
        let open = |url: &str| Outcome::Open(url.into());
        assert_eq!(enter(&mut s, "im the matrix"), open("https://www.imdb.com/find/?q=the%20matrix"));
        assert_eq!(enter(&mut s, "matrix"), open("https://www.google.com/search?q=matrix"));
        assert_eq!(
            enter(&mut s, "ok rynek"),
            open("https://www.openstreetmap.org/search?query=kraków+rynek#map")
        );
        assert_eq!(enter(&mut s, "a&b=c"), open("https://www.google.com/search?q=a%26b%3Dc"));
        assert_eq!(enter(&mut s, "im"), Outcome::Fill("im ".into()));
        assert_eq!(enter(&mut s, "hub"), Outcome::Fill("gh ".into()));
        assert_eq!(enter(&mut s, ""), Outcome::Fill("g ".into()));
        assert_eq!(enter(&mut s, "github.com/foo"), open("https://github.com/foo"));
        assert_eq!(enter(&mut s, "http://localhost:8080"), open("http://localhost:8080"));
    }

    #[test]
    fn ctrl_enter_takes_text_as_typed() {
        let mut s = search();
        s.update("hub");
        assert_eq!(s.accept("hub", Some(2), true), Outcome::Open("https://www.google.com/search?q=hub".into()));
        s.update("im  the matrix ");
        assert_eq!(
            s.accept("im  the matrix ", Some(1), true),
            Outcome::Open("https://www.imdb.com/find/?q=the%20matrix".into())
        );
        assert_eq!(s.accept("  ", None, true), Outcome::Nothing);
    }

    #[test]
    fn default_engine_goes_first() {
        let mut s = search();
        assert!(s.set_default("gh"));
        assert_eq!(s.set_default_index(3), Some("ok"));
        assert_eq!(s.set_default_index(9), None);
        let keywords: Vec<_> = s.engines.iter().map(|e| e.keyword.as_str()).collect();
        assert_eq!(keywords, ["ok", "g", "im", "gh"]);
        assert!(!s.set_default("nope"));
        assert_eq!(enter(&mut s, "rynek"), Outcome::Open("https://www.openstreetmap.org/search?query=kraków+rynek#map".into()));
        assert!(s.row(0).contains("· default"));
        assert!(!s.row(1).contains("· default"));
    }

    #[test]
    fn rows() {
        let mut s = search();
        assert_eq!(
            s.row(1),
            "<tt>im  </tt><b>IMDb</b><span alpha=\"60%\">   imdb.com</span>"
        );
        s.update("im <b>");
        assert_eq!(
            s.row(1),
            "<tt>im  </tt><b>IMDb</b><span alpha=\"60%\">   “&lt;b&gt;”</span>"
        );
    }

    #[test]
    fn addresses() {
        for yes in ["example.com", "a.b.co/x?y=1", "kraków.pl", "host.dev:8080/x", "https://x"] {
            assert!(is_address(yes), "{yes}");
        }
        for no in ["matrix", "1.5", "a.b c", "x.c", "host.dev:/x", "https://", "e.g"] {
            assert!(!is_address(no), "{no}");
        }
    }
}
