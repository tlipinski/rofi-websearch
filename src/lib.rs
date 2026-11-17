//! rofi mode that searches the web with keywords, like Firefox's search
//! shortcuts. See README.md.

// rofi's symbols only exist inside rofi, so tests build just the logic.
#[cfg(not(test))]
mod rofi;
mod search;
