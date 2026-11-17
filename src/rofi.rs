//! The rofi side: loading the engines and the default, and acting on Enter.

use std::collections::HashSet;
use std::ffi::{CStr, c_char};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs, ptr, thread};

use rofi_mode::{Action, Api, Event, Matcher, Style, cairo};

use crate::search::{Engine, Outcome, Search, host, parse_engines};

/// The engines file in `$XDG_CONFIG_HOME`, unless `-websearch-engines` is given.
const ENGINES_FILE: &str = "rofi-websearch/engines.ini";
/// Written to the engines file on the first start, and used when it can't be
/// read, so the plugin works out of the box.
const EXAMPLE_ENGINES: &str = include_str!("../engines.ini");
/// Command that opens a search, given the URL as its last argument.
const DEFAULT_OPEN_COMMAND: &str = "xdg-open";
/// Keyword of the default engine chosen with Alt+d, in `$XDG_STATE_HOME`.
const DEFAULT_ENGINE_FILE: &str = "rofi-websearch/default";
/// Downloaded favicons, named by host, in `$XDG_CACHE_HOME`.
const ICONS_DIR: &str = "rofi-websearch/icons";

struct WebSearch<'rofi> {
    api: Api<'rofi>,
    search: Search,
    /// From `-websearch-open`, e.g. `firefox -P home`.
    open_command: String,
}

/// The value of a rofi command line option like `-websearch-engines`.
fn arg(key: &CStr) -> Option<String> {
    let mut value: *mut c_char = ptr::null_mut();
    // SAFETY: rofi parses its own argv; the value points into it and stays
    // valid while rofi runs.
    let found = unsafe { rofi_mode::ffi::helper::find_arg_str(key.as_ptr(), &mut value) };
    if found == 0 || value.is_null() {
        return None;
    }
    // SAFETY: as above, a nul-terminated string from argv.
    Some(unsafe { CStr::from_ptr(value) }.to_string_lossy().into_owned())
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

/// `$XDG_CONFIG_HOME`, `$XDG_STATE_HOME` or `$XDG_CACHE_HOME`, with its
/// default under `~`.
fn xdg_dir(var: &str, default: &str) -> PathBuf {
    env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(default))
}

/// The engines file, and whether it was given with `-websearch-engines`.
fn engines_path() -> (PathBuf, bool) {
    match arg(c"-websearch-engines") {
        Some(path) => (expand_home(&path), true),
        None => (xdg_dir("XDG_CONFIG_HOME", ".config").join(ENGINES_FILE), false),
    }
}

/// Reads the engines file. On the first start, when the default one doesn't
/// exist yet, it's created from the example engines to be edited. A missing
/// `-websearch-engines` file isn't created, as it's more likely a typo.
fn read_engines(path: &Path, given: bool) -> String {
    let error = match fs::read_to_string(path) {
        Ok(text) => return text,
        Err(error) => error,
    };
    if error.kind() == ErrorKind::NotFound && !given {
        let created = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| {
            // create_new: never overwrite a file made since the read above.
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(EXAMPLE_ENGINES.as_bytes())
        });
        match created {
            Ok(()) => eprintln!("websearch: created {} from the example engines", path.display()),
            Err(error) => eprintln!("websearch: can't create {}: {error}", path.display()),
        }
    } else {
        eprintln!("websearch: {}: {error}, using the example engines", path.display());
    }
    EXAMPLE_ENGINES.to_string()
}

fn default_engine_path() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join(DEFAULT_ENGINE_FILE)
}

fn favicon_path(engine: &Engine) -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache").join(ICONS_DIR).join(host(&engine.url))
}

/// Downloads the favicons that aren't cached yet in the background. They show
/// up the next time rofi opens. DuckDuckGo's icon service is the fallback for
/// sites that block curl, e.g. Cloudflare's bot check on discogs.com. curl
/// writes to a .part file that's renamed when complete, so a failed download is
/// retried next time. A separate curl process
/// rather than an HTTP library because rofi exits right after a search, which
/// would kill download threads (see README.md).
fn fetch_favicons(engines: &[Engine]) {
    let mut hosts = HashSet::new();
    for engine in engines.iter().filter(|e| e.icon.is_none()) {
        let path = favicon_path(engine);
        if !hosts.insert(host(&engine.url))
            || path.exists()
            || path.parent().is_some_and(|dir| fs::create_dir_all(dir).is_err())
        {
            continue;
        }
        let host = host(&engine.url);
        let child = Command::new("sh")
            .args(["-c", r#"for url in "$2" "$3"; do curl -fsSL --max-time 10 -o "$1.part" "$url" && mv "$1.part" "$1" && break; done; rm -f "$1.part""#])
            .arg("sh")
            .arg(&path)
            .arg(format!("https://{host}/favicon.ico"))
            .arg(format!("https://icons.duckduckgo.com/ip3/{host}.ico"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            // Reaped in the background so it doesn't linger as a zombie.
            Ok(mut child) => drop(thread::spawn(move || child.wait())),
            Err(error) => eprintln!("websearch: curl: {error}"),
        }
    }
}

fn save_default_engine(keyword: &str) {
    let path = default_engine_path();
    let saved = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&path, format!("{keyword}\n")));
    if let Err(error) = saved {
        eprintln!("websearch: can't save {}: {error}", path.display());
    }
}

/// Runs `command` with the URL appended, through sh so the command can use
/// quotes, e.g. `firefox -P "my profile"`.
fn open_url(command: &str, url: &str) {
    let spawned = Command::new("sh")
        .args(["-c", &format!(r#"{command} "$1""#)])
        .arg("sh")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(error) = spawned {
        eprintln!("websearch: {command}: {error}");
    }
}

impl<'rofi> rofi_mode::Mode<'rofi> for WebSearch<'rofi> {
    const NAME: &'static str = "websearch\0";

    fn init(api: Api<'rofi>) -> Result<Self, ()> {
        let (path, given) = engines_path();
        let text = read_engines(&path, given);
        let (engines, warnings) = parse_engines(&text);
        for warning in warnings {
            eprintln!("websearch: {}: {warning}", path.display());
        }
        fetch_favicons(&engines);
        let mut search = Search::new(engines);
        // Without a saved one the first engine in the file is the default.
        if let Ok(keyword) = fs::read_to_string(default_engine_path()) {
            search.set_default(keyword.trim());
        }
        let open_command = arg(c"-websearch-open").unwrap_or(DEFAULT_OPEN_COMMAND.into());
        Ok(Self { api, search, open_command })
    }

    fn entries(&mut self) -> usize {
        self.search.len()
    }

    fn entry_content(&self, line: usize) -> rofi_mode::String {
        self.search.row(line).as_str().into()
    }

    fn entry_style(&self, _line: usize) -> Style {
        Style::MARKUP
    }

    /// The engine's `icon` (a path or an icon theme name), else its favicon
    /// once downloaded. rofi loads and caches it in the background.
    fn entry_icon(&mut self, line: usize, height: u32) -> Option<cairo::Surface> {
        let engine = &self.search.engines()[line];
        let icon = match &engine.icon {
            Some(icon) if icon.starts_with("~/") => expand_home(icon),
            Some(icon) => PathBuf::from(icon),
            None => Some(favicon_path(engine)).filter(|p| p.exists())?,
        };
        let request = self.api.query_icon(icon.to_str()?, height);
        self.api.retrieve_icon(request).ok()
    }

    fn preprocess_input(&mut self, input: &str) -> rofi_mode::String {
        self.search.update(input);
        input.into()
    }

    fn matches(&self, line: usize, _matcher: Matcher<'_>) -> bool {
        self.search.is_visible(line)
    }

    fn react(&mut self, event: Event, input: &mut rofi_mode::String) -> Action {
        let outcome = match event {
            // kb-custom-1 (e.g. `-kb-custom-1 Alt+d`): make the highlighted
            // engine the default, which moves it to the top.
            Event::CustomCommand { number: 0, selected: Some(selected) } => {
                if let Some(keyword) = self.search.set_default_index(selected) {
                    save_default_engine(keyword);
                }
                return Action::Reload;
            }
            Event::CustomCommand { number, .. } => return Action::SetMode(number.into()),
            Event::Ok { selected, .. } => self.search.accept(input.as_str(), Some(selected), false),
            Event::CustomInput { selected, .. } => self.search.accept(input.as_str(), selected, true),
            _ => return Action::Exit,
        };
        match outcome {
            Outcome::Open(url) => open_url(&self.open_command, &url),
            Outcome::Fill(text) => {
                *input = text.as_str().into();
                return Action::Reload;
            }
            Outcome::Nothing => {}
        }
        Action::Exit
    }
}

rofi_mode::export_mode!(WebSearch);
